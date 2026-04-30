# flow10 — M10: architecture & runtime flows

> Derived from `docs/idea.md` §6, §10 and the work10 plan.

## Component map

```
                +-------------------+
   keyboard --> | task::keyboard    |--(PgUp/PgDn/End)-> framebuffer scroll
                +---------+---------+
                          | (printable)
                          v
                  +---------------+      +---------------------+
                  |    shell      | ---> | framebuffer writer  |
                  +-------+-------+      +----------+----------+
                          |                         |
                          v                         v
              +---------------------+     +---------------------+
              |   VFS (multi-mount) |     |  scrollback ring    |
              +--+------+------+----+     +---------------------+
                 |      |      |
                 v      v      v
                FAT  tmpfs  devfs
                  \    |    /
                   v   v   v
                 +---------+
                 |  block  |  (devfs/disk0 → ata)
                 +----+----+
                      v
                    ATA PIO
```

## Mount tree at boot

```
/        FatFs    (data disk; was M9 default mount)
/tmp     TmpFs    (heap-backed; cleared every boot)
/dev     DevFs    (static node table)
/ram     RamFs    (legacy fallback if data disk missing)
```

## Boot sequence (deltas vs. M9)

1. Existing M9 init: GDT, IDT, paging, heap, framebuffer base.
2. **`framebuffer::init_scrollback(256)`** — allocates ring before banner.
3. ATA + block init (unchanged).
4. **`fs::mount("/",    FatFs::open(disk0))`** (replaces direct global FS).
5. **`fs::mount("/tmp", TmpFs::new())`**.
6. **`fs::mount("/dev", DevFs::new(console, serial0, disk0))`**.
7. Shell start (unchanged); reads VFS via longest-prefix lookup.

## File-write flow (`shell::cmd_write /foo "hello"`)

```
shell::cmd_write
  └─ vfs::resolve("/foo")        → (FatFs, "foo")
     └─ FatFs::create_file("foo")
          ├─ Fat::alloc_cluster()             (scan FAT1 for 0x0)
          ├─ Fat::write_chain(c, EOC, both_fats)
          ├─ Dir::add_entry(parent, name, c)
          └─ Block::write(cluster_lba, data)
```

## Scrollback flow

```
println!("...")                 # framebuffer writer
  ├─ ring.push(line)
  └─ if scroll_back == 0:
       blit(line)                # auto-scroll
     else:
       noop                      # frozen viewport

PgUp  → scroll_back = min(scroll_back + 1, ring.len() - viewport_rows)
        redraw_from_offset(scroll_back)
PgDn  → scroll_back = max(0, scroll_back - 1); redraw
End   → scroll_back = 0; redraw_from_offset(0)
```

## VFS resolve

```
vfs::resolve(path)
  longest_prefix(path) over mounts
  → (mount.fs, path[mount.prefix.len()..])
```

Examples:

- `/dev/zero` → `(DevFs, "zero")`
- `/tmp/log` → `(TmpFs, "log")`
- `/etc/motd` → `(FatFs, "etc/motd")`

## Failure paths

- ATA write fails mid-cluster → upper layer returns `Err`; shell prints error; FAT chain may be inconsistent until M16.
- Heap exhaustion in scrollback → ring drops oldest line; writer never panics.
- Mount table full → `mount` returns `Err(ENOSPC)`; surfaces to shell.
- Bad mount path (no leading `/`) → `mount` rejects with `Err(EINVAL)`.

## Data structures

```rust
struct Mount {
    prefix: &'static str,        // "/", "/tmp", "/dev"
    fs: Box<dyn FileSystem>,
}

struct ScrollbackRing {
    lines: VecDeque<Line>,       // capacity = 256 default
    viewport_rows: usize,
    scroll_back: usize,          // 0 = live tail
}

trait DevNode {
    fn read(&self, off: u64, buf: &mut [u8]) -> Result<usize>;
    fn write(&self, off: u64, buf: &[u8]) -> Result<usize>;
}
```
