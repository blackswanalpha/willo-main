# work24 — M24: Browser port (Ladybird-first, libc/libstdc++ shims, Qt-on-Wayland)

> Derived from `docs/idea.md` §10 (Graphics & GUI), §21 (Application Suite).

## Goal

Land the single largest porting effort: a real web browser. Pick **Ladybird** as the v1 target (self-contained deps via vcpkg: HarfBuzz, Skia, simdutf, libcurl + Qt for GUI/networking), with **Servo** as a fallback option behind a feature flag. Bring up sufficient libc/libstdc++ shims, port Qt's Wayland-native backend, expose JIT W^X exemptions through §39 sandbox profiles, and integrate with M22 GPU + M18 audio + M14 netstack.

## Depends on

- **M22** — GPU acceleration (Mesa Vulkan via Venus).
- **M19** — package manager + repo (browser ships as a `.willo` package).
- **M18** — audio (HTML5 `<video>` / `<audio>`).
- **M14** — TLS via rustls in userland.
- **M39** — sandboxing profile for the renderer process.

## Acceptance criteria

- [ ] `willo-browser https://example.com` paints the page in the compositor with selectable text.
- [ ] HTTPS works; certificate chain validates against the system CA store.
- [ ] HTML5 `<video>` plays an MP4 at 720p ≥ 24 fps using §40 codec framework.
- [ ] WebGL / `<canvas>` accelerated via Mesa Venus (CPU memcpy-on-frame counter == 0).
- [ ] The renderer process runs under a §39 MAC + seccomp profile; sandbox-violation tests confirm denial.
- [ ] `kernel/tests/browser_smoke.rs` boots Willo, launches the browser, loads a local fixture page, and screenshots match a reference within tolerance.
- [ ] Quarterly upstream-refresh agent (§19 update daemon) is configured with an open issue tracker.

## Task breakdown

### T1. libc shim — `userspace/libc-shim/`
- Subset of POSIX libc backed by Willo syscalls + `willoc` (M11+M12).
- Cover: stdio, file I/O, networking, threading (pthread mapped to Willo threads), signals, dynamic linker hooks.

### T2. libstdc++ port — `userspace/libstdcxx-shim/`
- Build libstdc++ using LLVM `libc++` first (smaller, simpler) — preferred path.
- Provide unwinder + RTTI via `libunwind` port.

### T3. vcpkg-equivalent — extend `willo-pkg` (M19)
- Manifest field `bundle = "vcpkg"` resolves transitively the way Ladybird expects.
- Build cache content-addressed alongside §19's regular pkg cache.

### T4. Qt Wayland backend port — `userspace/qt-willo/`
- Pin Qt 6.x; minimal subset (QtCore, QtGui, QtWidgets, QtNetwork, QtWayland).
- Wayland platform plugin uses `linux-dmabuf-v1` (M22 zero-copy path).

### T5. Ladybird integration — `userspace/willo-browser/`
- Vendored Ladybird at a pinned commit; build script orchestrating vcpkg + Qt + Mesa + libcurl.
- Window manager hints; clipboard via compositor primary selection.

### T6. JIT W^X policy — `kernel/src/mm/jit.rs` + §39 profile
- New `mprotect` flag: pages may transition `RW → RX` once with capability check.
- Per-process limit; logged to audit subsystem.

### T7. Network integration — `userspace/willo-browser/net.rs`
- DNS via `getaddrinfo`-shim → §14 resolver.
- TLS via rustls (no OpenSSL); HTTP/2; HTTP/3 if QUIC available.

### T8. Media path — `userspace/willo-browser/media.rs`
- Use §40 `willova` codec framework for `<video>` decode.
- §18 audio for `<audio>`; pipe through PipeWire-class daemon (§40).

### T9. Quarterly refresh task — config in §15 update daemon
- Pin commit; open a PR weekly that bumps Ladybird to a tested upstream snapshot.

## New / modified files

| Path | Change |
| --- | --- |
| `userspace/libc-shim/` | **new** |
| `userspace/libstdcxx-shim/` | **new** |
| `userspace/qt-willo/` | **new** |
| `userspace/willo-browser/` | **new** (vendored Ladybird) |
| `kernel/src/mm/jit.rs` | **new** |
| `userspace/willo-pkg/` | + vcpkg-bundle support |

## Tests to add

- `userspace/libc-shim/tests/posix_subset.rs` — POSIX conformance subset.
- `userspace/qt-willo/tests/wayland_hello.rs` — Qt window opens, paints, exits.
- `kernel/tests/browser_smoke.rs` — boot + browser + local fixture page + screenshot diff.
- `userspace/willo-browser/tests/sandbox_deny.rs` — renderer cannot read `/home/$user/.ssh/`.

## Risks & open questions

- **Scope** — months of porting; budget Q-by-Q, not weeks. Track in a `BROWSER-PORT.md` punchlist.
- **Ladybird stability** — pre-1.0 in 2026; pin commits and run the upstream test suite as a regression gate.
- **JIT exemption surface** — every W^X relaxation widens the kernel's attack surface; gate behind a per-process capability checked against §39 profile.
- **Qt size** — libqt + dependencies will dwarf the kernel; make sure `.willo` package format handles >100 MB binaries (compress + content-address).
- **Servo fallback** — Servo has been backgrounded; do not block on it. Track as `BROWSER-SERVO.md` if a contributor is keen.
- **Codec licensing** — H.264/HEVC patents may force regional repos; default ship has free codecs (VP9/AV1) only.
