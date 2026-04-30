# work16 — M16: WilloFS native journaling FS, procfs/sysfs, snapshots, journal MVP

> Derived from `docs/idea.md` §6 (Filesystem & Storage), §19 (Observability), §20 (Backup/Sync).

## Goal

Replace FAT32 as the root filesystem with a native, journaled, snapshot-capable FS (`WilloFS`). Land `procfs` + `sysfs` for runtime introspection. Ship the **structured system journal MVP** (binary records on disk) since it depends on the same FS primitives.

## Depends on

- **M15** — SMP + per-CPU + atomic refcounts (FS structures need them).
- **M13** — slab allocator, AHCI.

## Acceptance criteria

- [ ] `WilloFS` formatted on a partition; data + metadata journaled.
- [ ] Crash injection during write → after reboot, FS recovers cleanly (`fsck -n` passes).
- [ ] Copy-on-write subvolumes; `willo-snap create /home @snap-2026-04-29` returns instantly.
- [ ] `procfs` mounted at `/proc`; lists processes, `/proc/<pid>/{status,maps,fd}`.
- [ ] `sysfs` mounted at `/sys`; surfaces `/sys/class/{net,block,input}` and `/sys/firmware/acpi/tables`.
- [ ] System journal mounts at `/var/log/journal/`; `journalctl`-class CLI tails records.
- [ ] FAT32 still mountable for interop; root migrates from FAT to WilloFS at install time.

## Task breakdown

### T1. WilloFS on-disk layout — `kernel/src/fs/willofs/format.rs` (new)
- Superblock (magic, version, uuid, root inode, journal head/tail, generation).
- B-tree of inodes; B-tree of extents per inode.
- 4 KiB block size; 64-bit inode numbers.
- CRC32C on every block.

### T2. Journal — `kernel/src/fs/willofs/journal.rs`
- Physical block log; transaction = (begin, [block writes…], commit).
- Recovery: replay any complete txn, discard incomplete trailing one.
- Group commits (batch under one fsync).

### T3. CoW + snapshots — `kernel/src/fs/willofs/snap.rs`
- Reference-counted blocks.
- `snapshot(subvol)` clones the subvol root B-tree (copy single root pointer; refs do the rest).
- `unlink` on shared block decrements refcount; only frees when 0.

### T4. VFS upgrade — `kernel/src/fs/mod.rs`
- Promote VFS to support inode + dentry caches (M10's mount table was a placeholder).
- Switch root (`/`) from FAT to WilloFS at boot.

### T5. procfs — `kernel/src/fs/procfs.rs` (new)
- Synthesised on read; per-process directory snapshot at lookup time.
- Files: `status`, `maps`, `fd/N`, `cmdline`, `comm`.
- Non-PID files: `cpuinfo`, `meminfo`, `mounts`, `version`.

### T6. sysfs — `kernel/src/fs/sysfs.rs` (new)
- Reflects driver-model objects: `class`, `bus`, `devices`.
- Read-only by default; specific writable knobs gated per-attribute.

### T7. Journal MVP — `kernel/src/observ/journal.rs` (new) + `userspace/journalctl/`
- Append-only binary records: `(boot_id, ts, prio, unit, fields)`.
- Stored in WilloFS at `/var/log/journal/<boot_id>.jrn` (rotation by size + time).
- Kernel `printk`-class macros tee into the journal *and* serial.
- `journalctl --boot/--unit/--priority` CLI.

### T8. Snapshot CLI — `userspace/willo-snap/`
- `willo-snap create <path> <name>`, `list`, `delete`, `mount`.

## New / modified files

| Path | Change |
| --- | --- |
| `kernel/src/fs/willofs/{format,journal,snap,btree,inode}.rs` | **new** |
| `kernel/src/fs/procfs.rs` | **new** |
| `kernel/src/fs/sysfs.rs` | **new** |
| `kernel/src/fs/mod.rs` | inode/dentry cache, switch root |
| `kernel/src/observ/journal.rs` | **new** |
| `kernel/src/main.rs` | mount procfs, sysfs, journal at boot |
| `userspace/journalctl/`, `userspace/willo-snap/` | **new** |
| `src/main.rs` (runner) | format root partition with WilloFS |

## Tests to add

- `kernel/tests/willofs_basic.rs` — mkfs + mount + create/read/write/unlink.
- `kernel/tests/willofs_crash.rs` — abort QEMU mid-write; replay; verify integrity.
- `kernel/tests/willofs_snap.rs` — snapshot, mutate live, snapshot keeps original.
- `kernel/tests/procfs_status.rs` — `/proc/<self>/status` reports correct PID + state.
- `kernel/tests/sysfs_class.rs` — virtio-net device shows up under `/sys/class/net/`.
- `kernel/tests/journal_record.rs` — kernel log emits → journalctl finds record.

## Risks & open questions

- **FS bug = data loss** — be aggressive about CRCs and journal replay tests.
- **B-tree complexity** — keep nodes small (4 KiB); restrict to single-level locking for v1.
- **Journal size** — bound at 64 MiB rotating; rotation policy in `journald.conf`-class file.
- **Live FS upgrade from FAT** — out of scope; install-time migration only.
