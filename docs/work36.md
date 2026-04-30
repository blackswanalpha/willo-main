# work36 — M36: App suite, utilities (settings, system monitor, calculator, archive, screenshot, image viewer, media player)

> Derived from `docs/idea.md` §21 (Application Suite).

## Goal

Round out the day-one app set with the small but essential utilities that make a desktop OS feel complete: a unified **settings panel**, a **system monitor**, a **calculator**, an **archive manager**, a **screenshot tool**, an **image viewer**, and a **media player** layered on §40's codec framework.

## Depends on

- **M17** — compositor.
- **M30** — UTF-8 + locale formats.
- **M40** — codec framework (media player).
- **M16** — procfs/sysfs (system monitor).

## Acceptance criteria

- [ ] `willoc-settings` opens, exposes tabs: User, Network, Display, Sound, Power, Updates, Privacy, A11y.
- [ ] Each tab is a plugin; new domains can register without recompiling the host.
- [ ] `willomonitor` shows CPU/memory/disk/network/temperature graphs at 1 Hz with <5% CPU at idle.
- [ ] Per-process tree view; kill / nice / cpu-affinity actions via `willosudo`.
- [ ] `willocalc` handles basic + scientific + programmer modes; supports current §30 locale number formatting.
- [ ] `willoarchive` creates/extracts zip, tar, tar.gz, tar.zst; integrates with file manager via "Compress…"/"Extract here".
- [ ] `willoshot` captures screen / window / region; saves PNG; pastes to clipboard.
- [ ] `willoview` opens PNG, JPEG, WebP, GIF, AVIF; basic crop/rotate/resize; fast slideshow.
- [ ] `willoplay` plays MP4/MKV/WebM via §40 codec framework; subtitle support (SRT/ASS).
- [ ] All apps keyboard-navigable + AT-SPI accessible (§31).

## Task breakdown

### T1. Settings host + plugin ABI — `userspace/willoc-settings/`
- Each plugin is a `.willo` package providing a `SettingsPlugin` trait impl + UI surface.
- Default plugins: User, Network, Display, Sound, Power, Updates, Privacy, A11y.

### T2. System monitor — `userspace/willomonitor/`
- 1 Hz poll of procfs/sysfs: aggregate + per-process; ringbuffer 5 min.
- Graphs via `iced-willo` canvas; history hover-scrub.

### T3. Calculator — `userspace/willocalc/`
- Modes: basic, scientific, programmer (hex/dec/bin/oct), unit converter.
- §30 locale-aware decimal separator + grouping.

### T4. Archive manager — `userspace/willoarchive/`
- zip, tar, tar.gz, tar.zst via Rust crates.
- Drag-and-drop with `willofiles` (M34).

### T5. Screenshot — `userspace/willoshot/`
- Compositor screencopy protocol (M22 dma-buf import).
- Modes: full / window / region. Annotate (text, arrow, blur) before save.

### T6. Image viewer — `userspace/willoview/`
- Decoders via `image-rs` + AVIF (libavif port).
- Crop/rotate/resize; non-destructive then "Save as".

### T7. Media player — `userspace/willoplay/`
- Demux (mp4/mkv/webm) → §40 `willova` decode → audio mixer + compositor present.
- Subtitle pipeline: SRT/ASS render to compositor surface above video.

### T8. Notification daemon — `userspace/willoc-notify/` (also used by M35)
- D-Bus `org.freedesktop.Notifications`-class API.
- Toast surface via compositor; per-app filtering in §36 Settings/Privacy.

## New / modified files

| Path | Change |
| --- | --- |
| `userspace/willoc-settings/` | **new** |
| `userspace/willomonitor/` | **new** |
| `userspace/willocalc/` | **new** |
| `userspace/willoarchive/` | **new** |
| `userspace/willoshot/` | **new** |
| `userspace/willoview/` | **new** |
| `userspace/willoplay/` | **new** |
| `userspace/willoc-notify/` | **new** |
| `userspace/willoc-comp/screencopy.rs` | **new** protocol |

## Tests to add

- `userspace/willomonitor/tests/poll_under_5pct.rs` — idle CPU < 5%.
- `userspace/willocalc/tests/locale_format.rs` — `de_DE` uses `,` decimal.
- `userspace/willoarchive/tests/roundtrip.rs` — pack/unpack each format.
- `userspace/willoshot/tests/dmabuf_capture.rs` — captured bytes match a known fixture.
- `userspace/willoview/tests/avif.rs` — decode reference AVIF.
- `userspace/willoplay/tests/sub_render.rs` — ASS overlay positions correctly.

## Risks & open questions

- **procfs schema drift** — pin a parser version; integration test fails on schema change.
- **AVIF decoder size** — libavif is heavy; consider rust-native `ravif`/`libavif-rs`.
- **Screencopy security** — capturing other apps must require permission portal (§39).
- **Subtitle ASS** — feature-rich; render only common subset v1; document.
- **Calc precision** — 64-bit float vs decimal — programmer mode needs bigint; use `num-bigint`.
- **Settings plugin ABI stability** — pin a v1 in `docs/spec/settings-plugin.md`.
