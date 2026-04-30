# flow22 — M22: architecture & runtime flows (GPU acceleration)

> Derived from `docs/idea.md` §10 and the work22 plan.

## Component map

```
        userspace
        +-------------------+        +-----------------------+
        |  Vulkan/GL app    |------->|     Mesa (Venus)      |
        +-------------------+        +-----------+-----------+
                                                 |
                                                 v ioctl SUBMIT / GEM_*
        +------------------------------------------------------+
        |                kernel DRM/KMS (M17 + M22)            |
        |  +-----+  +-------+  +------+  +-------+  +-------+  |
        |  | GEM |  | DMA-  |  | sync |  | submit|  | mode  |  |
        |  |     |  | BUF   |  | fence|  | queue |  | set   |  |
        |  +--+--+  +---+---+  +--+---+  +---+---+  +---+---+  |
        +-----+--------+----------+----------+----------+------+
              |        |          |          |          |
              +--------+----------+----------+----------+
              |
              v
        +-----------------+   +-------------------+
        |  virtio-gpu 3D  |   |  intel-igpu (opt) |
        +--------+--------+   +---------+---------+
                 |                      |
                 v                      v
              QEMU host            real iGPU
              (Venus -> host
               Vulkan)
```

## GEM lifecycle

```
ioctl GEM_CREATE(size)
  -> alloc shmem-class object, pin pages
  -> alloc handle in process handle table
  <- handle u32

ioctl GEM_MMAP(handle)
  -> create VMA in process address space pointing at object pages
  <- mmap_offset

ioctl PRIME_HANDLE_TO_FD(handle)
  -> wrap GEM in dma-buf fd (refcount++)
  <- fd

(across process boundary via UNIX-socket SCM_RIGHTS — provided by M11/M12 IPC)

ioctl PRIME_FD_TO_HANDLE(fd)
  -> import dma-buf into target process's handle table (refcount++)
  <- handle u32

ioctl GEM_CLOSE(handle)
  -> handle table entry removed; if last ref, free pages
```

## Vulkan render flow (Venus)

```
app::vkQueueSubmit
  -> Mesa::Venus::encode(cmds)
  -> virtio-gpu virtqueue::submit
     -> kernel virtqueue tx
        -> QEMU host virglrenderer/Venus
           -> host Vulkan driver (e.g. RADV/anv)
              -> physical GPU
        <- host signals fence on virtqueue rx
     <- kernel marks fence::seq complete
  <- vkWaitForFences returns
```

## Compositor zero-copy present

```
client app                  willoc-comp                kernel DRM
   |                              |                          |
   | wl_surface.attach(buf=dmabuf-fd)----------------------->|
   | wl_surface.commit ---------->|                          |
   |                              | dmabuf import (PRIME) -->|
   |                              |   fence wait ----------->|
   |                              |   (no CPU memcpy)        |
   |                              | mode_atomic_commit() --->|
   |                              |                          |
   | <-- frame callback ----------|<- vblank IRQ ------------|
```

## Fence cross-process wait

```
producer:
  ioctl SUBMIT(gem_list, out_fence_fd)
   -> queues work; returns fence-fd
  send(fence_fd) over UNIX socket

consumer:
  recv(fence_fd)
  ioctl SYNC_IOC_WAIT(fence_fd, timeout)
   -> blocks until kernel signals fence
   -> wakes consumer
```

## Failure paths

- **GEM create OOM** → ioctl returns `ENOMEM`; userspace falls back to smaller buffer.
- **dma-buf import on closed fd** → `EBADF`; logged at WARN once per process.
- **Submit on stale fence** → `EINVAL`; submission rejected without queueing.
- **Venus host disconnect** (QEMU restart) → all in-flight fences signalled with error; Mesa surfaces `VK_ERROR_DEVICE_LOST`.
- **Intel iGPU hang** (bare metal) → watchdog after 2 s resets engine; pending submissions error out.

## Data structures

```rust
pub struct GemObject {
    pub id: u32,
    pub size: usize,
    pub backing: GemBacking,            // Shmem | Pinned | Imported(DmaBuf)
    pub refcount: AtomicU32,
}

pub struct DmaBuf {
    pub ops: &'static dyn DmaBufOps,
    pub size: usize,
    pub refcount: AtomicU32,
}

pub trait DmaBufOps: Sync {
    fn map(&self, off: u64, len: usize) -> Result<&[u8], Errno>;
    fn mmap(&self, vma: &mut Vma) -> Result<(), Errno>;
    fn attach(&self, dev: &Device) -> Result<DmaBufAttach, Errno>;
}

pub struct Fence {
    pub ctx: u64,
    pub seq: u64,
    pub state: FenceState,              // Pending | Signalled | Error(Errno)
    waiters: Mutex<Vec<Waker>>,
}

pub struct GpuSubmission {
    pub ctx: GpuContextId,
    pub gems: Vec<GemHandle>,
    pub cmd_buf: GemHandle,
    pub out_fence: Arc<Fence>,
}
```
