# work17 — M17: Display layer (KMS), mouse, compositor MVP, terminal-as-window

> Derived from `docs/idea.md` §10 (Graphics & GUI), §7 (Drivers — input).

## Goal

Replace the text-only framebuffer with a proper display stack: a KMS-style mode-setting interface, mouse + improved keyboard input, a Wayland-ish compositor (`willoc-comp`), and the existing shell rehosted as a windowed terminal app.

## Depends on

- **M16** — sysfs (publishes display + input objects), journal (for crash dumps from compositor).
- **M12** — IPC (compositor talks to clients via UNIX sockets).

## Acceptance criteria

- [ ] KMS device at `/dev/dri/card0` exposes connectors, modes, framebuffers.
- [ ] virtio-gpu driver (software fallback initially) does mode-setting + flips.
- [ ] PS/2 + USB-HID mouse delivers events through `evdev`-class `/dev/input/event*`.
- [ ] Compositor draws root background, cursor, and a single decorated window at boot.
- [ ] `willoc-term` runs as a userspace client, hosting the M11 shell.
- [ ] At least 2 windows simultaneously; click-to-focus; close button works.
- [ ] HiDPI scaling factor honoured per output.

## Task breakdown

### T1. DRM/KMS subsystem — `kernel/src/gfx/drm.rs` (new)
- `Connector`, `Encoder`, `Crtc`, `Plane`, `Framebuffer` objects.
- ioctl-style operations (over a syscall family).
- Event queue (vblank, hotplug) read by userspace.

### T2. virtio-gpu driver — `kernel/src/drivers/virtio/gpu.rs` (new)
- 2D mode first; 3D deferred.
- `RESOURCE_CREATE_2D`, `RESOURCE_ATTACH_BACKING`, `SET_SCANOUT`, `RESOURCE_FLUSH`, `TRANSFER_TO_HOST_2D`.

### T3. Input subsystem — `kernel/src/input/mod.rs` (new)
- Generic `InputDevice` trait; backends: existing keyboard, new PS/2 mouse, USB-HID (M18 will fully wire xHCI).
- evdev-style packed event records exposed via `/dev/input/event*`.

### T4. Compositor — `userspace/willoc-comp/` (new)
- Wayland-ish protocol over a UNIX socket at `/run/willoc/wayland-0`.
- Surface registry, buffer attach, damage tracking, frame callbacks.
- Cursor plane, top-level window decorations.

### T5. GUI client lib — `userspace/lib/willoc-client/` (new)
- Thin Rust crate; surface/buffer/event helpers.

### T6. Terminal — `userspace/willoc-term/` (new)
- Embeds the M11 shell as a child via `pty` syscalls (added in this milestone).
- Bitmap font (reuse `noto-sans-mono-bitmap`), scrollback (reuse the M10 ring as a userspace lib).

### T7. PTY layer — `kernel/src/tty/pty.rs` (new)
- Master/slave pair; line discipline (cooked vs. raw).
- Backs the terminal app and any future `ssh`.

## New / modified files

| Path | Change |
| --- | --- |
| `kernel/src/gfx/drm.rs` | **new** |
| `kernel/src/drivers/virtio/gpu.rs` | **new** |
| `kernel/src/input/{mod,kbd,mouse}.rs` | **new** (kbd reuses M9 driver) |
| `kernel/src/tty/pty.rs` | **new** |
| `kernel/src/syscall.rs` | + `openpty`, `ioctl(DRM_*)`, input read |
| `userspace/willoc-comp/` | **new** |
| `userspace/lib/willoc-client/` | **new** |
| `userspace/willoc-term/` | **new** |
| `kernel/src/main.rs` | start compositor + term as user processes |

## Tests to add

- `kernel/tests/drm_modeset.rs` — set 1024×768 mode on virtio-gpu.
- `kernel/tests/input_mouse.rs` — synthetic mouse events readable via `/dev/input/event*`.
- `kernel/tests/pty_roundtrip.rs` — write to master, read from slave, line discipline.
- `kernel/tests/compositor_smoke.rs` — boot, compositor up, screenshot via DRM dump.

## Risks & open questions

- **Software-only rendering** — fine for MVP; GPU acceleration in M20+.
- **Wayland protocol scope** — implement just `wl_compositor`, `wl_surface`, `wl_buffer`, `wl_seat`, `xdg_shell`. Anything else deferred.
- **Input event timestamping** — must use HPET monotonic from M15, not wall clock.
- **Compositor crash** — kernel keeps running; recovery: respawn compositor; user data lives in client apps' files, not compositor state.
