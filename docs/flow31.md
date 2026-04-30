# flow31 — M31: architecture & runtime flows (Accessibility)

> Derived from `docs/idea.md` §16 and the work31 plan.

## Component map

```
        +----------------------+
        |  user apps           |
        |  (iced-willo /       |
        |   egui-willo / etc.) |
        +----+---------+-------+
             | a11y tree (D-Bus)
             v
        +-----------------------------+
        | willoc-comp                 |
        |  org.a11y.atspi.Registry    |
        +----+--------+---------------+
             |        |
             v        v
        willo-a11y  themes (high-contrast,
        screen      large-text, dyslexia)
        reader
             |
             v
        willo-tts (espeak-ng)
             |
             v
        §18 audio core -> speakers
```

## Session bring-up

```
PAM session start (M37)
  -> systemd-class user session sets DBUS_SESSION_BUS_ADDRESS
  -> willoc-comp launched
       -> claims a11y registry on session bus
  -> autostart willo-a11y (if user.preference.screen_reader = on)
  -> autostart willo-osk (if no physical keyboard)
```

## Focus → utterance

```
user presses Tab
  -> compositor moves focus
  -> emits org.a11y.atspi.Event.Focus, args=(app, path)
willo-a11y receives:
  -> queries node properties: name, role, state
  -> formats utterance:
       "Save button" or "OK button, focused"
  -> willo-tts.speak(utterance)
  -> audio core mixes; output to speakers
total latency < 100 ms ideal
```

## State change (e.g. checkbox toggle)

```
user activates checkbox
  -> widget toggles state; emits Property.Changed(state, "checked")
  -> a11y bus broadcasts
willo-a11y:
  -> "checked" or "unchecked"
```

## Theme switch

```
§36 settings → theme = high-contrast
  -> compositor reloads theme manifest
  -> emits theme.changed signal
  -> all clients re-render with new palette + font scale
  -> willo-osk updates layout colours too
```

## Magnifier

```
Ctrl+Win+= pressed
  -> compositor magnify::zoom(1.25x)
  -> per-output transform applied (linear) to the surface texture
  -> redraw: same dma-bufs, sampled with scale
  -> mouse cursor scaled in inverse to keep feel
```

## On-screen keyboard

```
text input focus event
  -> compositor: no_phys_kbd && setting.osk_when_no_kbd
  -> launch willo-osk
willo-osk:
  -> reads locale from §30 → layout file
  -> renders layout; on tap, sends synthetic key event via wl_seat
  -> typed text appears in target app via text-input-v3
```

## Keyboard-only navigation

```
Tab cycles focus across the tab order (defined per widget)
Shift+Tab reverse
Enter activates default
Esc dismisses dialog
Arrow keys move within grids (file manager) or menus
F10 opens menu bar
Ctrl+F1 toggles a11y help mode (announces shortcuts on hover)
```

## Failure paths

- **App fails to expose AT-SPI** → screen reader speaks "(unlabelled element, role=button)"; logs once.
- **TTS engine crash** → screen reader marks engine "unavailable", surfaces toast; falls back to beep cues.
- **a11y bus unreachable** (early boot) → retries with backoff; never blocks login.
- **Widget tree explosion** → screen reader rate-limits per element to ≤1 utterance/sec.
- **OSK overlap with focused field** → compositor anchors OSK below focus rect; falls back to top if no room.

## Data structures

```rust
pub struct A11yNode {
    pub path: DBusPath,
    pub role: Role,                      // Button, CheckBox, TextField, ...
    pub name: String,                    // accessible_name
    pub description: Option<String>,
    pub state: A11yState,                // bitset: Focused | Selected | Checked | Pressed | Disabled
    pub parent: Option<DBusPath>,
    pub children: Vec<DBusPath>,
}

pub trait Accessible {
    fn accessible_node(&self) -> A11yNode;
    fn on_focus(&mut self, focused: bool) {}
    fn on_state_change(&mut self, st: A11yState) {}
}

pub struct Theme {
    pub id: ThemeId,
    pub palette: Palette,                // bg, fg, accent, border, ...
    pub font_scale: f32,
    pub contrast: Contrast,              // Default | High | UltraHigh
}

pub struct OskLayout {
    pub locale: LocaleId,
    pub rows: Vec<Vec<OskKey>>,
}
```
