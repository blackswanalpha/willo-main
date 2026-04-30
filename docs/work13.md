# work13 — M13: PCI enumeration, APIC routing, AHCI driver, swap, slab allocator

> Derived from `docs/idea.md` §5 (Memory Management), §7 (Device Driver Framework).

## Goal

Replace hard-coded I/O (ATA PIO, PIC, linked-list heap) with the building blocks every modern OS uses: PCI bus enumeration, I/O APIC + MSI-X interrupt routing, an AHCI SATA driver, swap-to-disk, and a slab + buddy allocator.

## Depends on

- **M12** — scheduler/processes/IPC. Swap and bigger drivers need preemption.

## Acceptance criteria

- [ ] PCI config space readable; full bus enumeration prints to journal.
- [ ] I/O APIC routes IRQs; legacy 8259 PIC fully masked.
- [ ] MSI-X works for at least one device (AHCI).
- [ ] AHCI driver reads + writes data disk under QEMU `-device ahci`.
- [ ] Existing FAT tests pass against AHCI-backed block device.
- [ ] Slab allocator backs `Vec`/`Box`/`String` heap; old linked-list allocator retired.
- [ ] Buddy allocator manages physical frames at page granularity.
- [ ] Swap to a dedicated partition: under memory pressure, anon pages migrate out and back in.

## Task breakdown

### T1. PCI enumeration — `kernel/src/pci/mod.rs` (new)
- MMIO config (MCFG from ACPI) preferred; fall back to legacy `0xCF8/0xCFC`.
- Walk bus 0..255, dev 0..31, fn 0..7.
- Build a `Device` registry with vendor/device/class/BARs.

### T2. I/O APIC + MSI-X — `kernel/src/apic.rs`, `kernel/src/pci/msix.rs` (new)
- Parse MADT for I/O APIC base + GSI ranges (uses M15's ACPI table parser; for M13 hard-code from QEMU values, switch in M15).
- Program redirection table.
- MSI-X capability discovery + vector allocation in PCI device.

### T3. AHCI driver — `kernel/src/drivers/ahci.rs` (new)
- HBA control + port registers via BAR5.
- Command list + FIS receive setup per port.
- `ahci_read`/`ahci_write` use NCQ-light: one outstanding command per port for simplicity.
- Implement the `block::BlockDevice` trait so VFS keeps working.

### T4. Slab allocator — `kernel/src/mm/slab.rs` (new)
- Per-size-class caches (8, 16, 32, …, 4096 B).
- Magazine-style per-CPU cache (single CPU until M15).
- `GlobalAlloc` impl replaces `linked_list_allocator`.

### T5. Buddy allocator — `kernel/src/mm/buddy.rs` (new)
- Manages physical frames, orders 0..15 (4 KiB to 128 MiB).
- Replaces `BootInfoFrameAllocator`.

### T6. Swap — `kernel/src/mm/swap.rs` (new)
- Dedicated swap partition (set up in QEMU runner: a second 64 MiB disk).
- Slot allocator (1 slot = 1 page).
- LRU-ish reclaim: clock algorithm walks page tables, evicts oldest anon page.
- `#PF` on swapped page: alloc frame, read from swap, remap.

## New / modified files

| Path | Change |
| --- | --- |
| `kernel/src/pci/mod.rs` | **new** |
| `kernel/src/pci/msix.rs` | **new** |
| `kernel/src/apic.rs` | + I/O APIC redirection |
| `kernel/src/drivers/ahci.rs` | **new** |
| `kernel/src/mm/slab.rs` | **new** |
| `kernel/src/mm/buddy.rs` | **new** |
| `kernel/src/mm/swap.rs` | **new** |
| `kernel/src/allocator.rs` | swap to slab; legacy code removed |
| `kernel/src/memory.rs` | use buddy for frame alloc |
| `src/main.rs` (runner) | add second `-drive` for swap disk |

## Tests to add

- `kernel/tests/pci_enum.rs` — finds AHCI controller, ATA controller.
- `kernel/tests/ahci_rw.rs` — round-trip a 4 KiB block.
- `kernel/tests/slab_stress.rs` — alloc/free pattern; no leaks.
- `kernel/tests/swap_roundtrip.rs` — pin process, evict, fault back, verify content.

## Risks & open questions

- **MSI-X off in QEMU defaults** — runner must pass `-machine q35` so MSI-X is available.
- **Slab fragmentation** — accept it for M13; revisit if benchmarks complain.
- **Swap thrashing** — only triggered under deliberate pressure tests for now; OOM killer arrives in M16-ish.
- **AHCI errata** — keep to the minimum spec; vendor quirks deferred.
