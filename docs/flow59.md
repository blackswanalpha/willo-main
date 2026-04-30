# flow59 — cross-cutting: GPU frame end-to-end (app → present)

> Cross-cutting flow citing M17, M22, M24, M30, M31, M40.

How a single visible frame travels from "an app calls a draw API" to "pixels light up", and why the system tries to keep the CPU memcpy counter at zero on the hot path.

## High-level pipeline

```
app draws frame N
  -> toolkit (iced-willo / Qt) constructs scene
  -> Mesa (Vulkan or GL via Zink)
  -> kernel DRM submit (§22)
  -> GPU executes
  -> presents via dma-buf
  -> compositor composes
  -> KMS scanout (§17)
  -> display refresh (60/90/120 Hz)
```

## Detailed sequence (Vulkan render to compositor present)

```
T+0   app::draw():
        record cmd buffer (vkCmdBeginRenderPass, vkCmdBindPipeline, ...)
        vkQueueSubmit -> wait fence + signal fence
T+~µs  Mesa Venus encoder converts to virtio-gpu cmd stream
T+~µs  ioctl SUBMIT to /dev/dri/card0:
        - GEM handle list
        - cmd buffer GEM handle
        - in_fence (wait), out_fence (signal)
T+~µs  kernel submit::queue:
        - validate handles
        - enqueue in per-context ringbuffer
        - call driver::submit
T+~µs  driver writes virtqueue desc; rings doorbell
        QEMU host receives -> virglrenderer Venus -> host Vulkan
        host GPU executes
        host signals virtqueue rx; out_fence becomes Signalled
T+1f   app receives signaled fence; queues present:
        wl_surface.attach(buffer = exported dma-buf via PRIME)
        wl_surface.frame(cb)
        wl_surface.commit
T+1f   willoc-comp:
        on commit, imports dma-buf via PRIME_FD_TO_HANDLE
        composes scene tree:
           cursor surface
           panel
           windows (z-ordered)
           overlays (notifications)
        if dma-buf path available -> use it directly (no CPU memcpy)
        else -> fall back to CPU SHM blit
        atomic mode-set:
           drmModeAtomicCommit({plane state, crtc props, prop fences})
T+vbl KMS scanout fires at next vblank:
        primary plane <- compositor framebuffer
        cursor plane <- cursor surface
        overlay plane(s) <- video / fullscreen game (skip composition)
        signals out fence
T+vbl scanout pixels reach display
```

## Zero-copy path

```
app                    Mesa             kernel              compositor          display
 |  vkSubmit            |                |                    |                    |
 |--cmd buf-->          |                |                    |                    |
 |                      |--ioctl SUBMIT->|                    |                    |
 |                      |                |--virtqueue submit->|                    |
 |                      |  out_fence     |                    |                    |
 |                      |<------signal---|                    |                    |
 |  prime_handle_to_fd  |                |                    |                    |
 |--dma-buf fd--------->|                |                    |                    |
 |  attach + commit     |                |                    |                    |
 |<-------- wayland --->|                |---> compositor imports same dma-buf -->|
 |                      |                |                    |                    |
 |                      |                |                    |--atomic commit --->|
 |                      |                |                    |                    |--scanout-->
```

## When zero-copy doesn't work

```
fallbacks (CPU memcpy counter > 0):
  - software KMS path on M22 disable (debug build)
  - SHM path for non-GPU clients (legacy app via SHM buffer)
  - format mismatch (e.g. compositor wants ARGB8888, client gives YUV)
       -> inserts conversion blit
  - capture path (§40 willopipe screencast)
       -> needs a CPU-readable copy if portal client requested
```

## Multi-monitor

```
KMS reports outputs HDMI-A-1, eDP-1 each with mode list
compositor builds per-output framebuffer
each output has its own scanout fence + flip event
HiDPI: per-output scale factor (1, 1.5, 2)
scanout to wrong output? Plane assignment per-output
```

## Subtitle / video path (§40)

```
willoplay decode -> dma-buf with NV12
willoplay attaches surface to compositor
compositor:
  primary plane = page background
  overlay plane = video dma-buf (NV12 -> RGB on the fly via plane format conversion if HW supports)
  primary plane on top = subtitle overlay
A/V sync: present_at(target_pts) using KMS prop "VRR_ENABLED" if available
```

## Accessibility path (§31)

```
focus changes in app
  -> AT-SPI signal (D-Bus)
  -> compositor a11y bus broadcasts
  -> willo-a11y subscribes; reads new focus node
  -> willo-tts speaks
  -> §40 audio core mixes -> speakers
  (independent of GPU pipeline; no GPU work for screen reader)
```

## Failure paths

- **GPU device lost** → Mesa surfaces VK_ERROR_DEVICE_LOST; compositor gracefully removes affected surfaces.
- **Fence timeout** (>5 s) → driver resets context; surfaces marked "stale"; user sees brief flash.
- **dma-buf import fail** → fallback CPU blit; performance counter; logged once per surface.
- **vblank missed** (frame budget exceeded) → triple buffering hides single misses; chronic misses surface as jank.
- **Atomic commit reject** (invalid prop combo) → compositor reverts to last-good state; logs.

## Performance counters

```
§29 willotrace gpu profiles:
  - frame submit count
  - average submit-to-fence latency
  - vblank-to-vblank time (jank histogram)
  - dma-buf import count vs CPU blit count
§36 system monitor "GPU" tab shows:
  utilisation %, mem usage, presentation queue depth
```

## Tunables

- VRR (variable refresh rate) per output.
- Compositor target latency vs throughput (vsync vs immediate).
- Triple vs double buffering.
- HiDPI scale factor per output.
- Cursor on overlay plane vs composited.

## Test posture

- `kernel/tests/gpu_3d_smoke.rs` (§22) — triangle render.
- `kernel/tests/dmabuf_xproc.rs` (§22) — cross-process dma-buf.
- `userspace/willoc-comp/tests/zero_copy.rs` — CPU memcpy counter == 0 on hot path.
- `userspace/willoplay/tests/sub_render.rs` — subtitle overlay (§36).
- `kernel/tests/gpu_resume_redraw.rs` — survives §23 suspend.
