# work18 — M18: Audio (HDA), USB stack (xHCI + HID + MSC)

> Derived from `docs/idea.md` §7 (Drivers), §11 (Multimedia).

## Goal

Bring up real-hardware-class audio and USB. Intel HDA driver + a small audio server pipes per-app streams to the speakers; xHCI host controller driver + USB core + HID + MSC class drivers make USB keyboards, mice, and flash drives just work.

## Depends on

- **M17** — input subsystem (USB-HID feeds `/dev/input/event*`), DRM (audio server may want to render meters).
- **M13** — PCI + MSI-X.

## Acceptance criteria

- [ ] HDA controller discovered via PCI; codec enumerated.
- [ ] Userspace `aplay`-class can play a 16-bit 44.1 kHz WAV.
- [ ] Audio server (`willoc-aud`) mixes ≥2 streams, exposes per-app volume.
- [ ] xHCI controller initialised; root hub ports report device attach.
- [ ] USB-HID keyboard delivers events through `/dev/input/event*`.
- [ ] USB-HID mouse works alongside keyboard.
- [ ] USB Mass Storage flash drive is mountable; FAT32 + WilloFS round-trip work.
- [ ] Hot-plug: remove device → events posted to sysfs + journal.

## Task breakdown

### T1. xHCI driver — `kernel/src/drivers/usb/xhci.rs` (new)
- Operational registers, command + event rings.
- Address device, configure endpoint, transfer ring per endpoint.
- MSI-X for primary interrupter.

### T2. USB core — `kernel/src/drivers/usb/core.rs` (new)
- Device + interface + endpoint objects.
- Class driver registration; descriptor parsing.
- Hot-plug events to sysfs (`/sys/bus/usb`).

### T3. HID class — `kernel/src/drivers/usb/hid.rs` (new)
- Boot-protocol keyboard + mouse first; report descriptor parser later.
- Hands packed events to the M17 input core.

### T4. MSC class — `kernel/src/drivers/usb/msc.rs` (new)
- Bulk-only transport, SCSI READ(10)/WRITE(10) passthrough.
- Surfaces as `block::BlockDevice` named `/dev/sd<n>`.

### T5. HDA driver — `kernel/src/drivers/audio/hda.rs` (new)
- CORB/RIRB command rings.
- Stream descriptors with cyclic ring buffers.
- Codec enumeration (`Realtek ALC*`-class, sufficient for QEMU).

### T6. Audio core — `kernel/src/audio/mod.rs` (new)
- PCM substream abstraction (rate, channels, format).
- Mixer node graph; per-stream volume.

### T7. Audio server — `userspace/willoc-aud/` (new)
- UNIX socket protocol; clients submit PCM frames.
- Sums into hardware substream; resamples if needed (linear first, sinc later).

### T8. Userspace audio tools — `userspace/aplay/`, `userspace/arecord/`
- Smoke-test clients: play WAV, capture to WAV.

### T9. Mount automation — `userspace/automount/`
- Watches `/sys/bus/usb` for storage attach; mounts under `/media/<label>`.

## New / modified files

| Path | Change |
| --- | --- |
| `kernel/src/drivers/usb/{xhci,core,hid,msc}.rs` | **new** |
| `kernel/src/drivers/audio/hda.rs` | **new** |
| `kernel/src/audio/mod.rs` | **new** |
| `kernel/src/input/mod.rs` | + USB-HID backend |
| `userspace/willoc-aud/`, `userspace/aplay/`, `userspace/arecord/`, `userspace/automount/` | **new** |
| `src/main.rs` (runner) | `-device qemu-xhci`, `-device intel-hda`, `-device hda-duplex`, USB drives |

## Tests to add

- `kernel/tests/xhci_enum.rs` — controller + root hub up.
- `kernel/tests/usb_hid_kbd.rs` — synthetic key press → input event delivered.
- `kernel/tests/usb_msc_rw.rs` — sector round-trip on a USB drive.
- `kernel/tests/hda_play.rs` — play a tone, sample frames, assert checksum.
- `kernel/tests/audio_mix.rs` — two streams sum to expected output.

## Risks & open questions

- **xHCI is large** — implement just enough for HID + MSC; isoch (audio/video class) deferred.
- **HDA codec quirks** — start QEMU-only; add real-hardware quirks under conditional compile.
- **Resampler quality** — linear is OK for system sounds; PipeWire-class quality not required yet.
- **USB power/PM** — selective suspend deferred to §14 power milestone.
