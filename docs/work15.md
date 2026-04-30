# work15 — M15: UEFI boot, ACPI parsing, SMP bring-up

> Derived from `docs/idea.md` §3 (Boot & Firmware), §4 (Kernel Core).

## Goal

Move Willo from BIOS-only single-CPU to UEFI booting with a real ACPI parser and SMP (multiple application processors brought up via INIT-SIPI-SIPI). This is the hardware-readiness milestone for everything afterwards.

## Depends on

- **M14** — networking. (Independent, but ordering keeps the workload manageable.)

## Acceptance criteria

- [ ] UEFI image builds alongside the BIOS image (both shippable from the runner).
- [ ] OVMF firmware boots Willo on QEMU.
- [ ] GPT partitioning works on the disk; `parted`-equivalent userspace utility recognises layout.
- [ ] ACPI tables parsed: RSDP → XSDT → MADT, HPET, MCFG, FADT.
- [ ] All available APs brought up; per-CPU GDT/IDT/TSS/stacks.
- [ ] Scheduler (M12) runs run queues per CPU; load balancing across cores.
- [ ] CoW + slab + buddy made SMP-safe (atomic refcounts, per-CPU caches).
- [ ] HPET drives a stable monotonic clock (replaces M13's APIC-only timer source).

## Task breakdown

### T1. UEFI bootloader path — `bootloader = "uefi"`
- Add a second `bootloader` build (the crate already supports both).
- Runner emits `uefi.img`; QEMU runs with `-drive if=pflash,format=raw,unit=0,file=OVMF_CODE.fd`.
- Verify the framebuffer + memory map handoff still match what `kernel/src/main.rs` expects.

### T2. ACPI parser — `kernel/src/acpi/mod.rs` (new)
- RSDP discovery: UEFI handoff passes pointer; for BIOS, scan EBDA + 0xE0000.
- Walk RSDT/XSDT, validate checksums.
- Parse MADT (LAPIC IDs + I/O APIC base), HPET, MCFG (PCIe ECAM), FADT (power + reset).

### T3. SMP bring-up — `kernel/src/smp.rs` (new)
- Build per-CPU structures (`PerCpu { id, gdt, idt, tss, kernel_stack, current_task }`).
- AP trampoline: copy 16-bit real-mode startup code to a low page; transition real → protected → long mode.
- INIT-SIPI-SIPI sequence for each LAPIC ID from MADT.
- Each AP installs IDT, enables paging into the same kernel mappings, joins scheduler.

### T4. Per-CPU scheduler — `kernel/src/sched.rs`
- Per-CPU run queue.
- Steal queue: idle CPU pulls from busiest peer.
- Migrate cost: track last_cpu to avoid ping-pong.

### T5. SMP-safe primitives
- Replace `Mutex` (spin) with `IrqSafeMutex` where used in IRQ context.
- Atomic page refcounts; CoW path uses CAS.
- Slab: per-CPU magazines (already designed in M13; finish the SMP part here).

### T6. HPET monotonic clock — `kernel/src/time.rs` (new)
- Map HPET MMIO from FADT/MCFG.
- Expose `monotonic_now() -> Duration`.
- APIC timer remains preemption pulse; HPET is the time source.

### T7. GPT — `kernel/src/block/gpt.rs` (new)
- Parse primary + backup GPT.
- Expose partitions as named block devices `/dev/sda1`, `/dev/sda2`, …

## New / modified files

| Path | Change |
| --- | --- |
| `kernel/src/acpi/mod.rs` | **new** |
| `kernel/src/smp.rs` | **new** |
| `kernel/src/time.rs` | **new** |
| `kernel/src/block/gpt.rs` | **new** |
| `kernel/src/sched.rs` | per-CPU queues + stealing |
| `kernel/src/gdt.rs`, `kernel/src/interrupts.rs` | per-CPU structures |
| `kernel/src/mm/slab.rs` | per-CPU magazines (finish) |
| `kernel/src/memory.rs` | atomic refcounts |
| `Cargo.toml`, `src/main.rs` (runner) | UEFI image path + OVMF |
| `userspace/parted/` | **new** (GPT inspector/editor) |

## Tests to add

- `kernel/tests/uefi_boot.rs` — boots under OVMF, prints banner.
- `kernel/tests/acpi_madt.rs` — MADT enumerates the right LAPIC IDs.
- `kernel/tests/smp_bringup.rs` — N-1 APs reach idle.
- `kernel/tests/smp_scheduler.rs` — N CPU-bound tasks each pinned to a CPU.
- `kernel/tests/hpet_monotonic.rs` — clock advances and never goes backwards.
- `kernel/tests/gpt_parse.rs` — primary + backup match.

## Risks & open questions

- **AP startup races** — AP must not touch the bootstrap stack; allocate per-AP stacks before SIPI.
- **TLB shootdown** — needed any time a kernel mapping changes; implement lazy IPI scheme.
- **AML interpreter** — out of scope. We use static tables only; full ACPI namespace deferred.
- **OVMF availability** — runner must locate `OVMF_CODE.fd` from system or download it.
