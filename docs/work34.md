# work34 — M34: App suite, foundations (terminal, file manager, text editor)

> Derived from `docs/idea.md` §21 (Application Suite — Day-One Apps).

## Goal

Ship the three apps every developer wants on day one: a Wayland-class **terminal emulator** with tabs/true-color/Unicode (alacritty/foot/wezterm-class), a **file manager** with previews and network mounts (Nautilus-class), and a modal **text editor** with LSP + tree-sitter (Helix/Kakoune-class). They all sit on top of M11–M30 and consume §22 GPU, §28 network mounts, and §30 i18n.

## Depends on

- **M11/M12** — userspace + processes.
- **M17** — compositor + input.
- **M22** — GPU (zero-copy present).
- **M28** — cloud / network mounts (file manager).
- **M30** — UTF-8 + bidi (terminal + editor).

## Acceptance criteria

- [ ] `willoterm` opens, spawns shell via PTY, renders true-color escape sequences, supports tabs (Ctrl+T) and search.
- [ ] CJK + emoji width correctly rendered (`wcwidth`-aware).
- [ ] `willofiles` browses VFS root, navigates into network mounts (`~/Cloud/`), shows thumbnails for images.
- [ ] Drag-and-drop within `willofiles` and to `willoterm` (URI list).
- [ ] `willedit` opens any UTF-8 file ≤ 4 MiB in <100 ms; tree-sitter syntax for Rust/JS/Markdown; LSP client connects to `rust-analyzer`-class server.
- [ ] Modal editing (Helix-style) with chord-friendly key model.
- [ ] All three apps exposed via §31 AT-SPI (keyboard nav + screen reader).
- [ ] `kernel/tests/pty_basic.rs` round-trips bytes through master/slave.

## Task breakdown

### T1. PTY device class — `kernel/src/tty/pty.rs` (new)
- Master/slave pair allocator; line discipline (echo, canonical mode, signals).
- ioctls: `TIOCSWINSZ`, `TIOCGWINSZ`, `TIOCSCTTY`.

### T2. `willoterm` — `userspace/willoterm/`
- Compositor client; tabs; true-color; UTF-8 + width via `wcwidth`.
- Scrollback (10k lines); search; copy/paste via primary + clipboard.
- Optional GPU rendering via §22 (feature flag); CPU fallback default.

### T3. `willofiles` — `userspace/willofiles/`
- Toolkit: `iced-willo`. Hierarchical view + details pane.
- Thumbnail cache via separate worker process (CPU-bound).
- Network mount support via §28 `~/Cloud/`.
- Right-click "Open with" via §19 mime registrations.

### T4. `willedit` — `userspace/willedit/`
- Rope buffer; modal (Helix-class default; configurable).
- Tree-sitter syntax for top languages (Rust, JS/TS, Python, Markdown, Go, C/C++).
- LSP client; runs language servers in §39 sandbox.
- Quick-open (fuzzy file finder); workspace symbol search.

### T5. mime + xdg-open — `userspace/willoc-mime/`
- `xdg-open`-class command resolves URI scheme/mime to default app via §19 manifest.

### T6. drag-and-drop / clipboard — `userspace/willoc-comp/dnd.rs`
- Wayland `wl_data_device_manager`-class protocol.
- URI list, text, image MIME types.

## New / modified files

| Path | Change |
| --- | --- |
| `kernel/src/tty/pty.rs` | **new** |
| `kernel/src/syscall.rs` | `posix_openpt`, `grantpt`, `unlockpt` |
| `userspace/willoterm/` | **new** |
| `userspace/willofiles/` | **new** |
| `userspace/willedit/` | **new** |
| `userspace/willoc-mime/` | **new** |
| `userspace/willoc-comp/dnd.rs` | **new** |

## Tests to add

- `kernel/tests/pty_basic.rs` — open pair, write master, read slave, line discipline.
- `userspace/willoterm/tests/wcwidth.rs` — CJK width correct.
- `userspace/willofiles/tests/network_mount.rs` — browse `~/Cloud/dropbox/`.
- `userspace/willedit/tests/lsp_handshake.rs` — connects to a stub LSP server.
- `userspace/willoc-comp/tests/dnd_uri.rs` — drag URIs from one app to another.

## Risks & open questions

- **PTY flow control** — XON/XOFF deadlocks if app misbehaves; bound master-side buffer + drop oldest.
- **Toolkit choice** — `iced-willo` first; `egui-willo` parallel; document why one app uses which.
- **LSP server distribution** — third-party binaries; sandbox via §39; document path discovery.
- **Thumbnails** — CPU-bound; off-process to avoid blocking UI.
- **Editor performance** — rope ok up to ~100 MiB; over that, fall back to log-structured buffer (deferred).
- **Tree-sitter grammars** — vendor pinned grammar files; bumping is a breaking change for highlight themes.
