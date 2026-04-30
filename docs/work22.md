# work22 — M22: GPU acceleration (DRM/GEM, DMA-BUF, virtio-gpu Venus, Mesa, Intel iGPU)

> Derived from `docs/idea.md` §10 (Graphics & GUI).

## Goal

Promote the M17 KMS-only display layer into a real **DRM-class** stack. Add **GEM** GPU memory objects, **DMA-BUF** cross-device buffer sharing, **fences** for GPU/CPU sync, and a **virtio-gpu 3D (Venus)** path for QEMU. Port enough of Mesa to run Vulkan in-guest, plus a feature-flagged Intel iGPU driver for bare-metal smoke tests.

## Depends on

- **M17** — KMS, compositor, mouse, terminal-as-window.
- **M13** — PCI + MSI-X for binding the GPU.
- **M22** ↔ **M11** mmap surface (GEM uses it).

## Acceptance criteria

- [ ] `IOCTL_GEM_CREATE`, `GEM_MMAP`, `GEM_CLOSE`, `PRIME_HANDLE_TO_FD`, `PRIME_FD_TO_HANDLE` round-trip.
- [ ] A GEM buffer exported via `PRIME_HANDLE_TO_FD` can be imported in another process and the bytes match.
- [ ] virtio-gpu 3D context initialises against Venus host and reports a Vulkan 1.2 device.
- [ ] `kernel/tests/gpu_3d_smoke.rs` renders a triangle through Venus and reads back the framebuffer (MD5 stable across runs).
- [ ] `cargo run --features intel-igpu` brings up an Intel iGPU on bare metal far enough to mode-set + present a solid color.
- [ ] Compositor (M17) presents a client buffer via dma-buf zero-copy (no CPU memcpy on hot path).
- [ ] Fence wait + signal works across a process boundary (`SYNC_IOC_WAIT`).

## Task breakdown

### T1. GEM object manager — `kernel/src/gfx/drm/gem.rs` (new)
- Per-device handle table; refcounted; pinned in physical memory or backed by shmem-class anon mapping.
- Helpers: `gem_create(size, flags)`, `gem_mmap`, `gem_close`, `gem_pin`/`unpin`.

### T2. DMA-BUF — `kernel/src/gfx/drm/dmabuf.rs` (new)
- File-descriptor-shaped object with refcount + ops vtable (`map`, `unmap`, `mmap`, `attach`, `detach`).
- Export from GEM handle, import into another process's GEM table.

### T3. Sync objects + fences — `kernel/src/gfx/drm/sync.rs` (new)
- `Fence { ctx, seq }`; in-kernel signal callback; cross-process via fence-fd.
- `SYNC_IOC_WAIT`, `SYNC_IOC_MERGE` ioctls.

### T4. DRM IOCTL surface — `kernel/src/gfx/drm/ioctl.rs`
- Numbers + structs aligned to Linux DRM (cheap WSL-class compat later).
- Commands: `GEM_*`, `PRIME_*`, `MODE_*` (already partial in M17), `SUBMIT`, `WAIT`, `GET_PARAM`.

### T5. Command-buffer submission — `kernel/src/gfx/drm/submit.rs`
- Per-context ringbuffer of submissions; each carries a list of GEM handles + a fence to signal on completion.
- Per-driver `submit()` callback runs on the device.

### T6. virtio-gpu 3D backend — `kernel/src/gfx/drivers/virtio_gpu/`
- Existing 2D (M17) gains 3D context creation (`CTX_CREATE`, `CTX_DESTROY`, `RESOURCE_CREATE_3D`).
- Venus encoded Vulkan command stream forwarded via virtqueue to `virglrenderer`/Venus host.

### T7. Mesa port — `userspace/mesa-willo/`
- Build Mesa with Willo target triple (relies on M33 stage0 cross compile).
- Vulkan loader + Venus driver; OpenGL via Zink-on-Vulkan.

### T8. Intel iGPU driver (feature-flagged) — `kernel/src/gfx/drivers/intel/`
- Mode-set + framebuffer first; 3D submission stubbed; deliberately scoped down to "boot + present" for v1.

### T9. Compositor zero-copy path — `userspace/willoc-comp/dmabuf.rs`
- Wayland-class `wl_drm`/`linux-dmabuf-v1` protocol.
- Composite client surfaces directly from imported dma-bufs; no CPU blit.

## New / modified files

| Path | Change |
| --- | --- |
| `kernel/src/gfx/drm/gem.rs` | **new** |
| `kernel/src/gfx/drm/dmabuf.rs` | **new** |
| `kernel/src/gfx/drm/sync.rs` | **new** |
| `kernel/src/gfx/drm/ioctl.rs` | **new** (extends M17) |
| `kernel/src/gfx/drm/submit.rs` | **new** |
| `kernel/src/gfx/drivers/virtio_gpu/*` | extend for 3D |
| `kernel/src/gfx/drivers/intel/*` | **new** (feature `intel-igpu`) |
| `userspace/mesa-willo/` | **new** (port + Cargo wrapper) |
| `userspace/willoc-comp/dmabuf.rs` | extend compositor |

## Tests to add

- `kernel/tests/gem_lifetime.rs` — refcount, close, leak detection.
- `kernel/tests/dmabuf_xproc.rs` — export in proc A, import in proc B, byte equality.
- `kernel/tests/fence_xproc.rs` — fence signalled across processes.
- `kernel/tests/gpu_3d_smoke.rs` — triangle render via Venus, MD5 stable.
- `userspace/willoc-comp/tests/dmabuf_present.rs` — zero-copy composite (CPU memcpy counter == 0).

## Risks & open questions

- **dma-buf lifetime** — premature free crashes hard; ship a refcount sanitizer test on day one.
- **CPU/GPU coherency** — flush/invalidate APIs must be explicit; choose between coherent + WC mappings carefully.
- **Mesa size** — porting Mesa is large; gate behind `--features mesa` and document a "no-Mesa" build path that still presents 2D.
- **Intel iGPU scope** — bare-metal Intel is best-effort; promotion to "supported" needs M23 (PM) + M40 (HW video).
- **Vulkan vs OpenGL** — Vulkan first; OpenGL via Zink to avoid two driver ports.
