# flow30 — M30: architecture & runtime flows (Localization)

> Derived from `docs/idea.md` §16 and the work30 plan.

## Component map

```
        userspace
        +-----------------+    +---------------------+
        | apps (term,     |    |  willo-locale-data  |
        | edit, mail, ...) |   |  (.willo package)   |
        +--------+--------+    +----------+----------+
                 | text input            ^
                 v                       |
        +------------------+    +--------+----------+
        | willoc-comp:     |    | willo-icu (bidi,  |
        | text-input-v3    |    | shape, fmt, fold) |
        +------+--------+--+    +-------------------+
               |        |
               v        v
        IME engines  HarfBuzz shaping ─→ fontdue/Skia raster ─→ surface
        (pinyin,
         kana, ...)
```

## Text rendering pipeline

```
string "你好 שלום العالم"
  -> willo-icu::bidi::shape_paragraph
       -> embedding levels per codepoint (UAX #9)
       -> resolve weak / neutral / strong
       -> output: [
            Run { script=Hani, level=0, text="你好" },
            Run { script=Hebr, level=1, text="שלום" },
            Run { script=Arab, level=1, text="العالم" },
          ]
  -> for each run:
       font = font_select(script, locale)
       glyphs = harfbuzz::shape(font, text, script, level)
       for glyph in glyphs:
         raster = fontdue.raster(glyph)
         compose to surface at x,y (advance vectors)
```

## IME flow (pinyin)

```
keystroke n -> compositor
  -> active IME = pinyin
  -> pinyin engine receives "n"
  -> trie partial match; candidates: 你 (nǐ), 那 (nà), 能 (néng), ...
  -> compositor shows candidate window (popup surface)
keystroke i -> "ni"
  -> candidates narrow: 你 (nǐ), 尼 (ní), ...
keystroke space -> commit candidate 你
  -> text-input-v3::commit_string("你") to focused surface
  -> app receives committed text via wl_keyboard pipeline
```

## RTL handling in editor

```
willedit displays "Hello العالم"
  -> bidi paragraph shape
  -> rendered display order:
       [LTR run][RTL run]
  -> cursor logic uses logical position (not visual)
       arrow-left moves to logical previous codepoint
       arrow-right moves to logical next codepoint
       (visual movement is consistent because shaper places runs)
```

## Locale fallback example

```
app requests t("greeting")
  -> locale_chain = ["zh_Hant_HK", "zh_Hant", "zh", "en"]
  -> for loc in chain:
       if data[loc].contains("greeting"):
            return data[loc]["greeting"]
  -> default to en
```

## UTF-8 path round-trip

```
write file "/home/lin/笔记.md"
  -> shell tokenises path as bytes
  -> VFS resolve: UTF-8 valid; canonicalise NFC
  -> FAT/WilloFS writes bytes verbatim (no truncation)
  -> readdir returns same bytes
  -> shell prints; framebuffer renders via shaper
```

## CLDR data load

```
app starts
  -> dlopen org.willo.LocaleSvc
  -> requests locale "ja_JP"
  -> service mmaps `willo-locale-data.willo` chunk for "ja_JP"
       -> plural rule, day names, number format
  -> service returns handle
```

## Failure paths

- **Invalid UTF-8 input** → decoder emits U+FFFD; never panics.
- **Missing locale** → fallback chain; final fallback `en`; warning once per request.
- **HarfBuzz shape returns 0 glyphs** (font missing script) → fallback font for script; tofu (□) only as last resort.
- **IME engine crash** → compositor unbinds engine; user sees "IME unavailable" toast; raw keystrokes resume.
- **Bidi level overflow** (>125 nesting) → algorithm caps; remaining levels treated as topmost.

## Data structures

```rust
pub struct ShapedRun {
    pub script: Script,                  // Hani, Hebr, Arab, Latn, ...
    pub level: u8,                       // 0=LTR, 1=RTL embedding
    pub text: SmolStr,
    pub glyph_ids: Vec<u32>,
    pub advances: Vec<i32>,
    pub clusters: Vec<u32>,              // map glyph -> char cluster
}

pub enum BidiClass {
    LeftToRight, RightToLeft, ArabicLetter, EuropeanNumber,
    EuropeanSep, EuropeanTerminator, ArabicNumber, CommonSep,
    NonSpacingMark, BoundaryNeutral, ParagraphSep, SegmentSep,
    Whitespace, OtherNeutral, LRE, LRO, RLE, RLO, PDF, LRI, RLI, FSI, PDI,
}

pub struct LocaleData {
    pub id: LocaleId,
    pub plural_rule: PluralRule,
    pub date_fmt: DateFormat,
    pub number_fmt: NumberFormat,
    pub day_names: [SmolStr; 7],
    pub month_names: [SmolStr; 12],
}

pub trait ImeEngine {
    fn key(&mut self, k: KeyEvent) -> ImeOutput;  // (preedit, commit, candidates)
    fn reset(&mut self);
    fn locale(&self) -> LocaleId;
}
```
