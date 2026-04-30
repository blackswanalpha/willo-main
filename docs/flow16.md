# flow16 — M16: architecture & runtime flows

> Derived from `docs/idea.md` §6, §19, §20 and the work16 plan.

## Component map

```
                    +-----------------------------+
   userspace -----> |          VFS                |
                    |  inode + dentry cache       |
                    +--+------+------+------+-----+
                       |      |      |      |
                       v      v      v      v
                  +-------+ +----+ +----+ +-----+
                  |WilloFS| |proc| |sys | |FAT  |
                  +---+---+ +----+ +----+ +-----+
                      |
                      v
              +-----------------+
              |   journal +     |
              |   B-tree mgr    |
              +--------+--------+
                       |
                       v
              +-----------------+
              |   block / AHCI  |
              +-----------------+

   journal-of-records (separate from FS journal):
     kernel printk! ─┐
     userspace logd ─┼──> /var/log/journal/<boot>.jrn  (append-only)
     audit subsys  ──┘                       │
                                              v
                                       journalctl
```

## WilloFS write path

```
file.write(off, buf)
  ├─ tx = journal.begin()
  ├─ extent = inode.extent_for(off, len)   (alloc if needed)
  ├─ tx.write(data_block, buf)
  ├─ tx.write(inode_block, updated inode)
  ├─ tx.write(extent_btree_blocks, updates)
  ├─ tx.commit()                           ; fsync barrier
  └─ return
```

## Crash recovery

```
mount(WilloFS):
  read superblock
  scan journal from sb.head:
    for each tx:
      if tx has commit record:  replay block writes
      else:                     discard (torn write)
  superblock.head = next_clean_offset
```

## Snapshot

```
snapshot("/home", "@snap-2026-04-29"):
  src = subvol_root("/home")
  new_root = copy_inode(src)         # one block copy
  refcount(src.children) += 1
  index.insert("/home/.snapshots/@snap-2026-04-29", new_root)
```

CoW kicks in lazily: any subsequent write to a shared block clones it.

## procfs synthesis

```
read("/proc/123/status"):
  proc = process_table.get(123)?
  out = format!("Pid: {} State: {:?} ...", proc.pid, proc.state)
  return out as bytes

readdir("/proc"):
  snapshot of process_table.keys() ++ ["cpuinfo","meminfo","mounts","version"]
```

No persistence; lookups race-free per-call.

## sysfs object tree

```
/sys
├── class/
│   ├── net/eth0          → driver virtio-net pci 0000:00:03.0
│   ├── block/sda         → driver ahci pci 0000:00:1f.2
│   └── input/input0      → driver i8042
├── bus/pci/devices/...
└── firmware/acpi/tables/{MADT,HPET,FACP,...}
```

Each leaf is a `KObject` with attributes (read closure + optional write closure).

## Journal record

```rust
struct Record {
    boot_id:   Uuid,
    timestamp: u64,        // ns since epoch
    priority:  u8,         // 0=emerg .. 7=debug
    unit:      [u8; 32],   // owning service
    fields:    Vec<(Key, Value)>,
}
```

Stored as length-prefixed CBOR-ish frames in `/var/log/journal/<boot>.jrn`.

## journalctl flow

```
journalctl --boot=current --priority=warn
  ├─ open all *.jrn for current boot_id
  ├─ scan records
  ├─ filter priority <= warn
  └─ pretty-print to stdout
```

## Failure paths

- Block CRC mismatch → mount marks superblock "needs check"; userspace `fsck` runs from recovery env (M20).
- Journal full → backpressure: synchronous flush of oldest txn before new ones accepted.
- procfs read on dead PID → `ENOENT`.
- sysfs write to read-only attr → `EACCES`.
- Journal disk full → drop oldest segment, log warning; never block the kernel printk path.
