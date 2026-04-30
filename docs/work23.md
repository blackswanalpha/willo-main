# work23 — M23: Power & thermal (ACPI S-states, pm_ops, cpufreq, battery, lid/backlight)

> Derived from `docs/idea.md` §14 (Power & Thermal).

## Goal

Make Willo a usable laptop OS. Implement ACPI **S0/S3/S4/S5** transitions, a Linux-shaped per-driver `pm_ops` contract, **cpufreq** + **cpuidle** governors, a battery + thermal sysfs surface, and lid/power-button/backlight handling wired to the compositor. The laptop user can close the lid, suspend in <1 s, resume in <1 s, and see correct battery percentage between cycles.

## Depends on

- **M15** — ACPI table parsing (FADT, MADT, DSDT/SSDT).
- **M13** — PCI driver model (every driver gains `pm_ops`).
- **M17** — compositor (consumes lid/backlight events).
- **M18** — HDA + USB drivers (must implement `pm_ops`).

## Acceptance criteria

- [ ] `echo mem > /sys/power/state` enters S3 in QEMU and resumes via fake wake source within 200 ms.
- [ ] `echo disk > /sys/power/state` produces a hibernation image on swap and resumes from it next boot.
- [ ] All in-tree drivers (ATA, AHCI, virtio-net, xHCI, HDA, virtio-gpu, NIC) implement `pm_ops` and pass `kernel/tests/suspend_resume.rs`.
- [ ] `/sys/class/power_supply/BAT0/capacity` reports a value 0..100 derived from ACPI `_BST`.
- [ ] `/sys/class/thermal/thermal_zone0/temp` reports a sane temperature.
- [ ] Lid close emits `LID_CLOSE` on the compositor event bus; default config triggers suspend.
- [ ] Backlight up/down keys change `/sys/class/backlight/.../brightness`; compositor responds within 50 ms.
- [ ] cpufreq governor `ondemand` reduces idle CPU clock; `cpuinfo` reports the new MHz.

## Task breakdown

### T1. ACPI sleep states — `kernel/src/pm/acpi_sleep.rs` (new)
- Resolve `\_S3`, `\_S4`, `\_S5` packages from DSDT.
- Execute `_PTS(state)` then write SLP_TYPa/SLP_ENa to PM1a_CNT.
- Reserve and write the wake vector in FACS; provide a real-mode-style trampoline that re-enters long mode.

### T2. `pm_ops` ABI — `kernel/src/pm/mod.rs` (new)
- `trait PmOps { fn suspend(&mut self), suspend_late, suspend_noirq, resume_noirq, resume_early, resume(&mut self) }`.
- Each driver in `kernel/src/drivers/*` registers a `Device` with optional `pm_ops`.
- DFS over device tree: pre-order on suspend, reverse-order on resume.

### T3. Userspace freeze/thaw — `kernel/src/pm/freeze.rs`
- `pm.freeze_userspace()` sends a SIGSTOP-class signal to every userspace task; waits for them to be scheduled out.
- `pm.thaw_userspace()` symmetric.

### T4. CPU state save/restore — `kernel/src/pm/cpu_state.rs`
- Save/restore: GSBASE/KGSBASE, MSRs (EFER/STAR/LSTAR/FMASK), CR0/CR2/CR3/CR4, IDTR/GDTR, FS/GS, FPU via XSAVE.
- Per-CPU LAPIC timer: capture deadline, restore as relative tick.

### T5. cpufreq + cpuidle — `kernel/src/pm/cpufreq.rs`, `kernel/src/pm/cpuidle.rs`
- cpufreq P-state driver via `MSR_IA32_PERF_CTL` (Intel) / `MSR_PSTATE_DEF` (AMD).
- Governors: `performance`, `powersave`, `ondemand`.
- cpuidle: HLT, MWAIT (C1/C2/C3) via `MWAIT` hints from ACPI `_CST`.

### T6. Battery + thermal — `kernel/src/pm/battery.rs`, `kernel/src/pm/thermal.rs`
- Read ACPI `_BIF`, `_BST`, `_TMP` per zone; expose under `/sys/class/power_supply/` and `/sys/class/thermal/`.

### T7. Input events — `kernel/src/pm/events.rs`
- Translate ACPI SCI events (`_LID`, `_PWRBTN`, `_BLI`) into compositor events.
- Backlight via ACPI `_BCL`/`_BCM` or vendor sysfs.

### T8. Hibernation image — `kernel/src/pm/hibernate.rs`
- Snapshot RAM pages → swap (M13) with a header + checksum.
- Resume detection at boot: if FACS hibernate signature present, restore image.

## New / modified files

| Path | Change |
| --- | --- |
| `kernel/src/pm/*` | **new** module |
| `kernel/src/drivers/*` | + `pm_ops` impls |
| `kernel/src/main.rs` | wire `pm::init()` |
| `userspace/willoc-comp/` | handle lid/backlight events |
| `Cargo.toml` (kernel) | enable `xsave` cpu feature |

## Tests to add

- `kernel/tests/suspend_resume.rs` — S3 + fake wake; verify scheduler clock + open files.
- `kernel/tests/hibernate.rs` — write hibernation image + resume.
- `kernel/tests/cpufreq_pstates.rs` — switch P-state, observe MSR change.
- `kernel/tests/lid_event.rs` — fake `_LID` SCI; assert compositor event.

## Risks & open questions

- **Driver `pm_ops` coverage** — any missing driver corrupts state on resume; gate per-driver "no-suspend" capability bit and warn on enable.
- **PCR brittleness w/ hibernation + TPM** — interacts with M38 FDE; defer secure-hibernate to M38.
- **CPU-state save** — easy to miss MSRs; build a regression test that diffs MSR snapshots pre/post suspend.
- **SMM/SMI interception** — vendor firmware writes to RAM during S3; relevant pages must be `nosave`.
- **HPET monotonic** — wall-clock skew on resume must not break timers; recompute base on resume.
