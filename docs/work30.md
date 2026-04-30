# work30 — M30: Localization (UTF-8 everywhere, ICU/CLDR-derived lib, IBus-class IME, RTL bidi)

> Derived from `docs/idea.md` §16 (Localization & Accessibility).

## Goal

Make Willo speak more than ASCII English. Audit and fix UTF-8 handling across kernel logs, shell, VFS path lib, and §17 framebuffer. Port a minimal ICU/CLDR-derived locale library, integrate **HarfBuzz** OpenType shaping into the M17 font path, build an **IBus-class IME framework** with one engine (pinyin or kana), and ship a **RTL bidi shaper** so Arabic/Hebrew renders correctly.

## Depends on

- **M17** — compositor + font rendering.
- **M19** — packaging (locale data ships as `.willo`).
- **M11** — userspace + IPC (IME bus).

## Acceptance criteria

- [ ] All kernel `println!`/`serial_println!` accept UTF-8 strings without truncation.
- [ ] Shell + VFS round-trip filenames containing CJK + emoji + diacritics.
- [ ] HarfBuzz integrated into the §17 font path; ligatures and combining marks render correctly.
- [ ] CLDR-derived data (locale names, plural rules, number/date formats) shipped as `willo-locale-data` package; ≥40 locales.
- [ ] IME framework provides D-Bus-class registration; pinyin engine attaches and produces composed text in `willoterm` and `willedit`.
- [ ] Arabic + Hebrew strings render right-to-left with correct shaping (initial/medial/final forms).
- [ ] `kernel/tests/bidi_unicode_conformance.rs` passes the UCD `BidiTest.txt` vectors.
- [ ] `kernel/tests/l10n_render_cjk.rs` renders a known CJK string and matches a reference glyph map.

## Task breakdown

### T1. UTF-8 audit — `kernel/src/`
- Replace any `&str` byte-truncating code with grapheme- or codepoint-aware operations.
- Add `Utf8Decoder` to serial console; emit U+FFFD on invalid bytes.

### T2. Locale data package — `userspace/willo-locale-data/`
- Build script ingests CLDR XML; emits compact binary tables (locale id → plural rule, number/date format, names).
- Ships as `.willo` package; consumed via D-Bus `org.willo.LocaleSvc`.

### T3. ICU-class library — `userspace/willo-icu/`
- Minimum surface: bidi (UAX #9), grapheme break (UAX #29), case folding, normalisation (NFC/NFD), collation (DUCET), number/date formatting.
- Pure Rust where possible; port small bits of ICU only where necessary.

### T4. HarfBuzz integration — `userspace/willoc-comp/text.rs`
- Replace fontdue+rasteriser-only path with HarfBuzz shaping → fontdue/Skia rasterisation.
- Cache shaped runs; invalidate on font change.

### T5. RTL bidi shaper — `userspace/willo-icu/bidi.rs`
- Implement UAX #9 algorithm; handle weak/neutral/strong directionality; embedding levels.
- Expose `shape_paragraph(text) -> Vec<Run>`.

### T6. IME framework — `kernel/src/input/ime.rs` + `userspace/willo-ime/`
- Wayland-ish `text-input-v3` protocol on the compositor side.
- D-Bus `org.willo.IME` for engines (pinyin, kana, libIME-class plugins).
- Engine registration + activation by current input language.

### T7. First IME engine — `userspace/willo-ime-pinyin/`
- Port a small open-source pinyin dictionary; trie + lookup; candidate window via compositor.

### T8. Locale fallback chain — `userspace/willo-icu/fallback.rs`
- `zh_Hant_HK -> zh_Hant -> zh -> en`.
- Used by every consumer of locale data.

## New / modified files

| Path | Change |
| --- | --- |
| `kernel/src/serial.rs` | UTF-8-aware writer |
| `kernel/src/framebuffer.rs` | route through HarfBuzz path |
| `kernel/src/input/ime.rs` | **new** |
| `userspace/willo-icu/` | **new** |
| `userspace/willo-locale-data/` | **new** |
| `userspace/willo-ime/` | **new** |
| `userspace/willo-ime-pinyin/` | **new** |
| `userspace/willoc-comp/text.rs` | HarfBuzz shaping |

## Tests to add

- `kernel/tests/utf8_path.rs` — VFS round-trip CJK + emoji filename.
- `kernel/tests/bidi_unicode_conformance.rs` — UCD `BidiTest.txt` (subset for runtime).
- `kernel/tests/l10n_render_cjk.rs` — render reference CJK; glyph-map equality.
- `userspace/willo-icu/tests/normalisation.rs` — NFC/NFD round-trip Unicode 16 vectors.
- `userspace/willo-ime/tests/pinyin_compose.rs` — `nihao` → 你好.

## Risks & open questions

- **ICU binary size** — minimise; consider a Rust-native `icu4x` path (preferred) over a port.
- **Bidi correctness** — subtle; gate behind UCD conformance test.
- **HarfBuzz licensing** — MIT-OK; confirm at vendor time.
- **CLDR data churn** — pin a CLDR version; bump quarterly via §15 update daemon.
- **IME latency** — must be <30 ms keystroke-to-candidate; profile via §29 PMU.
- **Compositor protocol stability** — `text-input-v3` not yet finalised in 2026; pin to a specific draft and document.
