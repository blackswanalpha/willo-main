# flow13 — M13: architecture & runtime flows

> Derived from `docs/idea.md` §5, §7 and the work13 plan.

## Component map

```
                  +----------------+
                  |  ACPI MCFG     |  (M15 will own this; M13 stub)
                  +-------+--------+
                          |
                          v
                 +------------------+
                 |  PCI enumerator  |
                 +---+----------+---+
                     |          |
                     v          v
              +-----------+ +-----------+
              | AHCI ctrl | | Other dev |
              +-----+-----+ +-----------+
                    |
                    v BAR5
              +-----------+
              |   AHCI    |---> block::BlockDevice
              +-----+-----+
                    |
                    v
              +------------+
              |   VFS      |  (unchanged)
              +------------+


  +------------+    +------------+    +------------+
  | I/O APIC   |--->|  MSI-X tbl |--->|  IDT vec   |
  +------------+    +------------+    +-----+------+
                                            |
                                            v
                                       handler
```

## Memory subsystem

```
       virtual addresses                physical frames
       +-----------+
       | user maps |---map(page)----+
       +-----------+                |
       | kernel    |                v
       +-----------+          +----------+
                              |  buddy   |   (M13 frame allocator)
                              +----+-----+
                                   |
                                   v
                              +----------+
                              |   slab   |   (kernel heap; replaces LL)
                              +----+-----+
                                   |
                                   v
                              GlobalAlloc

       swap path:
       evict victim (anon page, low refs)
         ├─ slot = swap.alloc()
         ├─ ahci.write(slot.lba, page)
         ├─ pte: present=0, swap=1, swap_idx=slot
         └─ buddy.free(frame)

       swap-in (#PF, swap bit set):
         ├─ frame = buddy.alloc()
         ├─ ahci.read(slot.lba, frame)
         ├─ swap.free(slot)
         └─ pte: present=1, swap=0
```

## PCI enumeration flow

```
for bus in 0..256:
  for dev in 0..32:
    for fn in 0..8:
      cfg = read_dword(bus, dev, fn, 0)
      if cfg.vendor != 0xFFFF:
         register Device {
           bdf, vendor, device, class,
           bars: read BARs,
           caps: walk capability list,
         }
```

## AHCI read flow

```
ahci.read(lba, buf, count)
  ├─ acquire port.lock
  ├─ slot = find_free_command_slot(port)
  ├─ build CFIS (H2D, READ_DMA_EXT) with lba/count
  ├─ build PRDT pointing at buf
  ├─ port.command_issue |= 1 << slot
  ├─ wait for IRQ (MSI-X) or poll port.command_issue clear
  ├─ check port.task_file_data for errors
  └─ release slot
```

## Interrupt routing

```
device --> MSI-X table[i] = (vector v, dest = lapic_id)
        \
         +--> CPU receives v in IDT
              IDT[v] = ahci_irq_handler
              handler.signal port.work_done
```

## Swap reclaim cycle

```
on memory_pressure (free < 5%):
  scan = clock_hand.next_n(256)
  for pte in scan:
    if !accessed: evict(pte)
    else:        clear_accessed(pte)
```

## Failure paths

- AHCI device error → port reset, command requeued; if 3× retries fail, surface `EIO`.
- Swap full → reclaim continues; if every page is dirty + pinned, OOM (M16).
- Slab corruption (double free) → debug build poisons memory + panics; release build best-effort.
- MSI-X unsupported → fall back to legacy line-based IRQ via I/O APIC.
