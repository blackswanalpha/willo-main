# work40 — M40: Multimedia (VA-API-class codec framework, V4L2 capture, PipeWire-class screen capture, MIDI)

> Derived from `docs/idea.md` §11 (Multimedia).

## Goal

Make Willo a real media OS. Land a **VA-API-class** hardware-accelerated codec framework (`willova`) covering H.264, HEVC, VP9, AV1; a **V4L2-class** camera capture stack under `/dev/video*`; a **PipeWire-class** daemon (`willopipe`) exposing the screen-capture portal; and a stretch **MIDI** sequencer over §18 audio core. Decoded frames flow as **dma-buf** to the compositor for zero-copy presentation.

## Depends on

- **M22** — DRM/GEM/dma-buf (zero-copy frame path).
- **M18** — audio core + HDA driver.
- **M39** — sandbox + portal authorisation.
- **M19** — codec packs ship as `.willo` plug-ins.

## Acceptance criteria

- [ ] `willova` decodes H.264, HEVC, VP9, AV1 in QEMU virtio-gpu HW path; software fallback covers all four.
- [ ] V4L2-class capture works: `/dev/video0` exposes `VIDIOC_REQBUFS`/`VIDIOC_DQBUF`; QEMU webcam stub feeds a test pattern.
- [ ] `willopipe` exposes D-Bus `org.freedesktop.portal.ScreenCast`; user prompt grants window/screen capture.
- [ ] Decoded video presents zero-copy through compositor (CPU memcpy counter == 0 on hot path).
- [ ] MIDI: a synthetic `aplaymidi`-class CLI plays a small `.mid` file via §18 audio core.
- [ ] All optional codecs ship as separate `.willo` packages; default install free codecs only.
- [ ] `kernel/tests/v4l2_capture.rs` reads frames from QEMU video device.
- [ ] `userspace/willova/tests/decode_av1.rs` decodes a known clip.

## Task breakdown

### T1. Codec framework `willova` — `kernel/src/media/willova/` + `userspace/willova/`
- Kernel hosts the dma-buf surface manager + HW driver glue.
- Userspace `libwillova` provides decode/encode API to apps; loads codec plug-ins.

### T2. SW + HW decoders — `userspace/willova/codecs/`
- SW: dav1d (AV1), libvpx (VP9), openh264 (H.264), libde265 (HEVC).
- HW: virtio-gpu video; Intel iGPU via §22 driver hooks (best effort).

### T3. V4L2-class kernel — `kernel/src/media/v4l2/`
- `/dev/video*` char dev; ioctls `VIDIOC_QUERYCAP`, `VIDIOC_ENUM_FMT`, `VIDIOC_S_FMT`, `VIDIOC_REQBUFS`, `VIDIOC_QBUF`, `VIDIOC_DQBUF`, `VIDIOC_STREAMON`/`OFF`.
- Buffer types: MMAP, DMABUF.

### T4. willopipe daemon — `userspace/willopipe/`
- D-Bus `org.freedesktop.portal.ScreenCast` + `org.willo.PipeWire`.
- Negotiates source (window or output), permission portal prompt, emits dma-buf stream to client.

### T5. Compositor screencopy + capture — `userspace/willoc-comp/screencopy.rs`
- Re-used by §36 willoshot and willopipe.
- Per-output and per-surface capture modes.

### T6. MIDI — `kernel/src/media/midi.rs`, `userspace/willomidi/`
- ALSA-seq-class API; soft synth (FluidSynth-class) or external USB-MIDI device passthrough.

### T7. Codec pack packaging — `userspace/willova-codecs-*/`
- Separate `.willo` packages per codec; license-flagged H.264/HEVC default off.

## New / modified files

| Path | Change |
| --- | --- |
| `kernel/src/media/willova/*` | **new** |
| `kernel/src/media/v4l2/*` | **new** |
| `kernel/src/media/midi.rs` | **new** |
| `userspace/willova/` | **new** |
| `userspace/willopipe/` | **new** |
| `userspace/willoc-comp/screencopy.rs` | **new** protocol |
| `userspace/willomidi/` | **new** |
| `userspace/willova-codecs-*/` | **new** plug-in packages |

## Tests to add

- `userspace/willova/tests/decode_h264.rs` — known clip, stable hash.
- `userspace/willova/tests/decode_av1.rs` — AV1 reference clip.
- `kernel/tests/v4l2_capture.rs` — capture N frames from QEMU webcam stub.
- `userspace/willopipe/tests/portal_prompt.rs` — portal grants → stream.
- `userspace/willomidi/tests/midi_play.rs` — `.mid` file → audio core.

## Risks & open questions

- **Codec licensing** — H.264/HEVC patents; default ship free codecs; document regional repos.
- **HW driver coverage** — virtio-gpu first; Intel iGPU best-effort; AMD/NVIDIA deferred.
- **V4L2 format zoo** — many pixel formats; pick a small canonical subset (NV12, YUV420, MJPEG) v1.
- **Portal sandbox** — willopipe must integrate with §39 portal; never grant capture without explicit user prompt.
- **Audio ↔ video sync** — A/V clock domain; willoplay (§36) is the canonical consumer; bench drift.
- **MIDI scope** — basic seq + soft synth; pro-audio (jack-class low-latency) deferred.
- **Memory pressure** — decoded frames are big; pipeline must use ringbuffer of dma-bufs and back-pressure on slow consumers.
