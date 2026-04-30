# work10 — M10: FAT32 write, framebuffer scrollback, tmpfs + devfs

> Derived from `docs/idea.md` §6 (Filesystem & Storage), §10 (Graphics & GUI), §22 (Roadmap).

## Goal

Finish the M9 FAT32 backend by adding a write path, replace the M8 "clear-on-overflow" framebuffer with a real scrollback ring, and introduce two virtual filesystems (`tmpfs`, `devfs`) so the VFS in `kernel/src/fs/mod.rs` hosts more than one mount.

## Depends on

- **M9** — current state: FAT32 read, ATA PIO, RamFs, 13 integration tests.

## Acceptance criteria

- [ ] `kernel/tests/fat_write.rs` fully covers create / append / delete and passes against a fresh data disk.
- [ ] After tests, `fsck.fat -nv` (run from QEMU runner) reports no errors.
- [ ] Shell scrolls with `PgUp` / `PgDn`; `End` snaps to live tail; ring keeps ≥256 lines.
- [ ] `/tmp` mount of `tmpfs` accepts files within a session and is empty after reboot.
- [ ] `/dev` mount of `devfs` exposes `console`, `null`, `zero`, `serial0`, `disk0`.
- [ ] VFS holds ≥4 mounts simultaneously; longest-prefix-match resolves correctly.

## Task breakdown

### T1. FAT32 write path — `kernel/src/fs/fat.rs`
- FAT cluster allocator: scan FAT1 for `0x00000000`, mark `0x0FFFFFFF` (EOC).
- Directory entry insertion (8.3 short names first; LFN later if time permits).
- Implement `create`, `write_at`, `append`, `truncate`, `unlink` on top of cluster ops.
- Mirror every FAT mutation to FAT2 in the same call.

### T2. Scrollback ring — `kernel/src/framebuffer.rs`
- `RingBuffer<Line>` in heap (default 256 lines × 80 cols).
- Writer appends to ring **and** the visible viewport when `scroll_back == 0`.
- `redraw_from_offset(offset)` rebuilds visible glyphs from the ring.

### T3. Scrollback key bindings — `kernel/src/task/keyboard.rs`
- Recognise `PgUp` / `PgDn` / `End` scancodes; route through framebuffer instead of shell line.
- `End` resets `scroll_back = 0`.

### T4. VFS mount table — `kernel/src/fs/mod.rs`
- Replace single-backend enum with `Vec<Mount { prefix: &str, fs: Box<dyn FileSystem> }>`.
- Lookup: longest-prefix-match.
- Helpers `mount(path, fs)` / `unmount(path)`; bound to ≤8 mounts.

### T5. `tmpfs` — `kernel/src/fs/tmp.rs` (new)
- Reuse the `RamFs` storage layer with an independent root.
- Cleared on each boot (no persistence hooks).

### T6. `devfs` — `kernel/src/fs/dev.rs` (new)
- Static node table mapping name → `&'static dyn DevNode`.
- Nodes: `console` (framebuffer writer), `null` (discard), `zero` (infinite zeros), `serial0` (UART), `disk0` (raw block device).

## New / modified files

| Path | Change |
| --- | --- |
| `kernel/src/fs/fat.rs` | + write path |
| `kernel/src/fs/mod.rs` | multi-mount VFS |
| `kernel/src/framebuffer.rs` | scrollback ring |
| `kernel/src/task/keyboard.rs` | PgUp/PgDn routing |
| `kernel/src/fs/tmp.rs` | **new** |
| `kernel/src/fs/dev.rs` | **new** |
| `kernel/src/main.rs` | mount `/tmp` and `/dev` at boot |

## Tests to add

- `kernel/tests/fat_write.rs` — flesh out the existing stub with create/append/delete cases.
- `kernel/tests/fb_scrollback.rs` — write 1024 lines, scroll back 100, verify content.
- `kernel/tests/tmpfs.rs` — write/read across two mounts.
- `kernel/tests/devfs.rs` — read 4 KiB of `/dev/zero`, write to `/dev/null`, write+read `/dev/serial0`.

## Risks & open questions

- **FAT corruption on panic mid-write** — accepted for M10; M16's WilloFS adds journaling.
- **Scrollback memory** — 256 × 80 × `Line` may exceed the current 100 KiB heap; bump heap to 256 KiB.
- **LFN entries** — defer to a follow-up if time tight; short names are enough to seed `/sbin/init` for M11.
- **Two-FAT sync** — must update both copies in the same call or `fsck` flags it.
