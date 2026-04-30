# work31 — M31: Accessibility (AT-SPI-class bus, willo-a11y screen reader, TTS, high-contrast/keyboard nav)

> Derived from `docs/idea.md` §16 (Localization & Accessibility).

## Goal

Make Willo usable by people with different abilities. Build an **AT-SPI2-class accessibility bus** layered on the compositor, with each app exposing an accessibility tree (label/role/state). Ship a screen reader **`willo-a11y`** that subscribes, plus a TTS pipeline that speaks via §18 audio. Add high-contrast + large-text themes, full keyboard navigation, and an on-screen keyboard for touch-only sessions.

## Depends on

- **M17** — compositor.
- **M18** — audio core (TTS output).
- **M30** — locale-aware text + bidi (TTS phonetics).
- **M37** — session manager (PAM hook to start a11y bus).

## Acceptance criteria

- [ ] D-Bus `org.a11y.atspi.Registry` claimed by compositor at session start.
- [ ] Every native toolkit (`iced`/`egui` ports) emits AT-SPI nodes for each interactive widget; `accessible_name` non-empty enforced at compile time where possible.
- [ ] `willo-a11y` reads focus changes; pronounces label + role + state via TTS.
- [ ] High-contrast and large-text themes selectable in §36 settings.
- [ ] Every GUI surface in the day-one app suite (§34/§35/§36) is fully usable by keyboard only (Tab/Shift-Tab/Enter/Esc/arrows).
- [ ] On-screen keyboard appears on focus-in to a text field when no physical keyboard is attached.
- [ ] `kernel/tests/a11y_speak.rs` (userspace integration) verifies screen reader pronounces a labelled button.
- [ ] AT-SPI tree updates within 100 ms of widget tree mutation.

## Task breakdown

### T1. Accessibility bus — `userspace/willoc-comp/a11y_bus.rs`
- Compositor claims `org.a11y.atspi.Registry` on a per-user D-Bus instance.
- Each app's accessible tree exposed under app-specific D-Bus path.

### T2. Toolkit AT-SPI implementations — `userspace/iced-willo/a11y.rs`, `userspace/egui-willo/a11y.rs`
- Walk widget tree; emit (path, label, role, state, parent) tuples.
- Hook focus + state-change signals.

### T3. Screen reader — `userspace/willo-a11y/`
- D-Bus subscriber to a11y bus; tracks focus, fires speech.
- Configurable verbosity (silent/brief/verbose).

### T4. TTS engine — `userspace/willo-tts/`
- Port `espeak-ng` (small, multilingual) initially; future: pluggable.
- Output PCM via §18 audio core.

### T5. High-contrast + large-text themes — `userspace/willoc-comp/themes/`
- Theme manifest format: colour palette + font scaling.
- "high-contrast", "large-text", "dyslexia-friendly" themes.

### T6. Keyboard navigation — `userspace/iced-willo/focus.rs`
- Focus model with explicit tab order; arrow keys for grids/menus.
- Compile-time lint: every interactive widget must declare focus order or opt-out.

### T7. On-screen keyboard — `userspace/willo-osk/`
- Compositor client; pops up on text-field focus when no physical keyboard.
- Locale-aware layouts (driven by §30 locale data).

### T8. Magnifier + DPI scaling — `userspace/willoc-comp/magnify.rs`
- Compositor-level zoom (Ctrl+Win+`+`/`-`).
- Per-output scale factor for HiDPI.

## New / modified files

| Path | Change |
| --- | --- |
| `userspace/willoc-comp/a11y_bus.rs` | **new** |
| `userspace/willoc-comp/themes/*` | **new** |
| `userspace/willoc-comp/magnify.rs` | **new** |
| `userspace/iced-willo/a11y.rs` | **new** |
| `userspace/egui-willo/a11y.rs` | **new** |
| `userspace/willo-a11y/` | **new** |
| `userspace/willo-tts/` | **new** |
| `userspace/willo-osk/` | **new** |

## Tests to add

- `userspace/iced-willo/tests/a11y_label.rs` — every interactive widget non-empty `accessible_name`.
- `userspace/willo-a11y/tests/focus_announce.rs` — focus change → expected utterance.
- `kernel/tests/a11y_speak.rs` — labelled button pronounced.
- `userspace/willoc-comp/tests/keyboard_nav.rs` — Tab sequence covers all widgets.
- `userspace/willo-osk/tests/locale_layout.rs` — French AZERTY shows when locale `fr_FR`.

## Risks & open questions

- **Custom widgets** — easy to forget AT-SPI; compile-time `#[derive(Accessible)]` attribute with required fields.
- **TTS quality** — espeak is robotic; pluggable to allow neural TTS later (§40 multimedia opens this).
- **Wayland a11y bus** — protocol still evolving; pin a draft and document.
- **Latency** — TTS lag of >300 ms is unusable; profile and budget.
- **i18n + a11y** — TTS voice availability per §30 locale; surface gaps in §36 settings.
- **Performance** — large widget trees (file manager) emit many AT-SPI signals; throttle to coalesce.
