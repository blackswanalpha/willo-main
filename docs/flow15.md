# flow15 — M15: architecture & runtime flows

> Derived from `docs/idea.md` §3, §4 and the work15 plan.

## Component map

```
   UEFI firmware (OVMF)
        │
        v
   bootloader (UEFI)  ─── framebuffer + memmap + RSDP ──> kernel
        │
        v
   kernel_main (BSP only)
        │
        ├─ acpi::parse(RSDP)  ──> tables { MADT, HPET, MCFG, FADT }
        ├─ smp::bringup(MADT.lapic_ids)
        │     for each AP: send INIT-SIPI-SIPI
        │     AP runs trampoline -> long mode -> ap_main()
        ├─ time::init(HPET)
        ├─ sched::add_cpu(N)        # per-CPU queues
        └─ enter idle on BSP

   per CPU:
   +--------------------------------------------------+
   | PerCpu { id, gdt, idt, tss, run_q, current }     |
   +--------------------------------------------------+
   |   APIC timer ── preempt ──> sched.tick(this_cpu) |
   +--------------------------------------------------+
```

## Boot sequence (UEFI path)

```
firmware
  └─ bootloader.efi
       ├─ exit_boot_services
       ├─ build BootInfo { framebuffer, memmap, rsdp_addr }
       └─ jump kernel_main(BootInfo)
```

## SMP bring-up

```
acpi.madt -> [lapic_ids]
for ap in lapic_ids except BSP:
  alloc ap.stack, ap.gdt, ap.idt, ap.tss
  ap.percpu = PerCpu::new(ap)
  copy 16-bit AP trampoline -> 0x8000
  lapic.send_INIT(ap)
  delay 10 ms
  lapic.send_SIPI(ap, vector = 0x08)
  delay 200 us
  lapic.send_SIPI(ap, vector = 0x08)   # spec: send twice
  wait until ap.percpu.online == true

ap_trampoline (asm @ 0x8000):
  real -> protected -> enable PAE/long -> jump ap_main(percpu)

ap_main(percpu):
  load gdt/idt
  load cr3 = kernel_pml4
  setup syscall MSRs
  percpu.online = true
  loop { schedule(this_cpu) }
```

## Per-CPU scheduler

```
sched::tick(cpu)
  if cpu.current.time_left == 0:
     cpu.run_q.push_back(cpu.current)
     next = cpu.run_q.pop_front()
     if next is None:
        next = steal_from_busiest()
     ctx_switch(cpu.current, next)

steal_from_busiest():
  victim = arg_max(other.run_q.len())
  if victim.len > 1: pop_back from victim.run_q, return
  else: return idle_task
```

## TLB shootdown

```
unmap(va) on cpu A
  ├─ flush local TLB for va
  └─ broadcast IPI (vector TLB_FLUSH) to all other cpus with this AS active
        each: invlpg va; ack
  └─ continue
```

## Time sources

```
+---------+        +-----------+
| HPET    |--->    | Monotonic |  (used for: timestamps, journal,
+---------+        +-----------+    timeouts, sched stats)

+---------+        +-----------+
| RTC     |--->    | Wall      |  (used for: filesystem mtimes, logs UI)
+---------+        +-----------+

+---------+        +-----------+
| LAPIC   |--->    | Preempt   |  (per-CPU 10 ms tick)
+---------+        +-----------+
```

## ACPI table walk

```
RSDP (from UEFI handoff) → XSDT (or RSDT)
  for ptr in xsdt.entries:
     hdr = *ptr
     match hdr.signature:
       "APIC" → MADT
       "HPET" → HPET
       "MCFG" → MCFG
       "FACP" → FADT
       _      → ignore
     verify checksum
```

## GPT parse

```
disk0
  ├─ LBA0  protective MBR
  ├─ LBA1  GPT header (signature, partition table CRC)
  ├─ LBA2.. partition entries (128 entries × 128 B by default)
  └─ tail: backup GPT
```

Each partition surfaces as a `BlockDevice` with name `sda<N>`.

## Failure paths

- AP fails to come online within 1 s → log + continue with the CPUs that did; BSP keeps running.
- ACPI checksum mismatch → table ignored, fall back to defaults; warn loudly.
- HPET missing → fall back to APIC-deadline mode; `monotonic_now` derived from TSC.
- GPT both copies corrupt → boot fails; recovery env (M20) intervenes.
