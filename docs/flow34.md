# flow34 — M34: architecture & runtime flows (App suite, foundations)

> Derived from `docs/idea.md` §21 and the work34 plan.

## Component map

```
        +-------------+   +-------------+   +-------------+
        |  willoterm  |   |  willofiles |   |  willedit   |
        +------+------+   +------+------+   +------+------+
               |                 |                 |
               v                 v                 v
            kernel PTY        VFS + §28          rope buf
            (line disc.)      mounts             tree-sitter
               |                 |                 |
               +--------+--------+--------+--------+
                        | §17 compositor + dnd + clipboard
                        v
                     surfaces
```

## willoterm bring-up

```
willoshell$ willoterm
  -> openpty() -> (mfd, sfd)
  -> fork:
       child: setsid; ioctl(sfd, TIOCSCTTY); dup2 sfd onto 0/1/2; exec /bin/sh
       parent: keep mfd
  -> compositor surface created
  -> read loop:
       on KEY: write to mfd
       on PTY MASTER readable: read bytes; UTF-8 decode; cell-grid update; redraw
```

## True-color escape

```
shell prints "\x1b[38;2;255;128;0mhello\x1b[0m"
  -> terminal parser:
       CSI 38;2;255;128;0 m -> set_fg_color(255,128,0)
       runs of text rendered in that color
       CSI 0 m -> reset attributes
```

## Tabs + search

```
Ctrl+T -> new tab; spawn another PTY/shell pair
Ctrl+Tab cycles tabs
Ctrl+F -> open search overlay; finds in scrollback
       arrows next/prev match
```

## willofiles browse + thumbnails

```
willofiles ~
  -> readdir VFS
  -> for entry:
       collect name/size/mtime/permissions
       if mime is image: enqueue thumbnail job
thumbnail worker (separate process, sandboxed §39):
  -> read file (bounded I/O)
  -> decode (image-rs)
  -> resize to 256px
  -> write to ~/.cache/willo/thumbs/{hash}.webp
  -> notify UI
willofiles redraws affected rows with thumbnails
```

## Network mount browse

```
~/Cloud/dropbox/photos
  -> §28 cloud daemon: backend.list("photos")
  -> entries appear as remote inodes (size, mtime, etag)
willofiles:
  -> on file activate:
       backend.get(path)  (lazy)
       open with mime-bound app via xdg-open
```

## willedit open + LSP

```
willedit src/lib.rs
  -> read file -> rope buffer
  -> tree-sitter parse (Rust grammar) -> syntax highlight
  -> spawn rust-analyzer in §39 sandbox
       initialize LSP; capabilities exchange
  -> on edit:
       rope.edit(range, text)
       tree-sitter incremental reparse
       LSP textDocument/didChange
       diagnostics arrive; gutter updates
```

## DnD URI from willofiles to willoterm

```
user drags file from willofiles
  -> willofiles offers MIME types: text/uri-list, text/plain
  -> compositor data-device-manager negotiates with willoterm
  -> willoterm accepts text/uri-list
  -> on drop: receive uri "file:///home/lin/笔记.md"
  -> writes path to PTY master (so shell sees it as typed)
```

## Clipboard

```
willoterm select region
  -> compositor PRIMARY selection set
willedit middle-click paste
  -> compositor PRIMARY selection delivers bytes
  -> willedit insert at cursor
Ctrl+C / Ctrl+V uses CLIPBOARD selection (different bus)
```

## Failure paths

- **PTY master full** → terminal drops oldest scrollback; emits visual notch.
- **Thumbnail decoder OOM** → worker killed by §39 limits; row falls back to generic icon.
- **Network mount unreachable** → file manager shows greyed entries with retry button.
- **LSP server crash** → editor reports "language server stopped"; restart button.
- **Editor undo stack >1k entries** → coalesce small edits; cap at 10k.

## Data structures

```rust
pub struct Pty {
    pub master: FileHandle,
    pub slave: FileHandle,
    pub winsize: WinSize,                // rows, cols, xpix, ypix
    pub line_discipline: LineDiscipline, // Canonical | Raw
    pub control_chars: [u8; 32],
}

pub struct TermGrid {
    pub rows: usize,
    pub cols: usize,
    pub cells: Vec<Cell>,                // wrap row*cols
    pub scrollback: VecDeque<Vec<Cell>>, // capacity 10_000
    pub cursor: (usize, usize),
    pub attrs: CellAttrs,
}

pub struct RopeBuffer {
    pub root: RopeNode,                  // balanced rope
    pub line_index: LineIndex,           // O(log n) line lookup
}

pub struct WorkspaceMime {
    pub default_apps: BTreeMap<String /* mime */, AppId>,
}
```
