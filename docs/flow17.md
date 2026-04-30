# flow17 — M17: architecture & runtime flows

> Derived from `docs/idea.md` §7, §10 and the work17 plan.

## Component map

```
   userspace
   +--------------+   +------------+   +-------------+
   | willoc-term  |   | other app  |   | willoc-comp |
   +-----+--------+   +-----+------+   +------+------+
         |                  |                 |
         +-----wl proto-----+                 |
                            |                 |
                          (UNIX socket /run/willoc/wayland-0)
                                              |
                                              v
   kernel                                  +----+-----+
                                           |   DRM    |
                                           +-+--+-+---+
                                             |  |
                                             v  v
                                          virtio-gpu  legacy fb
                                             |
                                             v
                                            GPU


   input path:
   PS/2 mouse / USB-HID  ─┐
                          ├──> input core ──> /dev/input/event*
   keyboard (M9)          ─┘                            │
                                                        v
                                               willoc-comp.dispatch
```

## Compositor runtime

```
willoc-comp:
  bind /run/willoc/wayland-0
  open /dev/dri/card0; pick connector; set 1920x1080@60
  alloc scanout framebuffer
  loop:
    poll(socket, drm_event, /dev/input/event*)
      socket: client request (attach buf, damage, commit)
      drm_event: vblank → frame callbacks fire
      input: dispatch to focused surface
    composite():
      for each surface in z-order:
         blit(surface.buffer, surface.geom)
      page-flip(scanout)
```

## Window lifecycle

```
client:
  wl_compositor.create_surface() → surface S
  wl_shm or virtio-gpu buffer alloc → buffer B
  S.attach(B, 0, 0)
  S.damage(0,0,w,h)
  xdg_surface(S).get_toplevel() → role
  S.commit()
compositor:
  add S to scene graph
  next vblank: composite + flip
```

## Input event flow

```
HW IRQ → driver
  → pack { dev, code, value, ts }
  → /dev/input/eventN ring (kfifo)

compositor.read("/dev/input/eventN")
  → resolve focused surface
  → wl_pointer / wl_keyboard event to client
```

## PTY round-trip

```
willoc-term  ── master fd ──>  pty master
                                  │
                                  v
                          line discipline (cooked: ICRNL, ECHO)
                                  │
                                  v
                              pty slave  <── slave fd ── /bin/sh
```

## Mode-setting sequence

```
ioctl(card0, DRM_GETRESOURCES) → list connectors/encoders/crtcs
choose connector (HDMI-1) + preferred mode (1920×1080@60)
ioctl(card0, DRM_GETCONNECTOR, id) → modes
fb_id = ioctl(card0, DRM_ADDFB2, {w,h,format,handles,pitches,offsets})
ioctl(card0, DRM_SETCRTC, {crtc, fb_id, x, y, connectors, mode})
loop {
  draw into back-buffer
  ioctl(card0, DRM_PAGEFLIP, {crtc, fb_id_back})
  wait DRM_EVENT_FLIP_COMPLETE
  swap buffers
}
```

## Failure paths

- KMS mode unsupported → fall back to nearest mode; warn in journal.
- Compositor crash → init respawns; clients see socket close, attempt reconnect.
- Client buffer corruption → compositor drops surface, sends `wl_protocol_error`.
- HiDPI mismatch (no integer scale) → use fractional scale via `wp_fractional_scale`.
