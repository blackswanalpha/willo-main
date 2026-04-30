# flow23 — M23: architecture & runtime flows (Power & thermal)

> Derived from `docs/idea.md` §14 and the work23 plan.

## Component map

```
        +-----------------+
        |  willoc-comp    |  (lid / backlight / power-btn UI)
        +--------+--------+
                 |  events
                 v
        +-----------------+      +-----------------+
        |  pm::events     |<-----+  ACPI SCI / GPE |
        +--------+--------+      +-----------------+
                 |
                 v
        +-----------------+      +-----------------+
        | pm core (state) |<---->| sysfs surfaces  |
        +--+----+----+----+      +-----------------+
           |    |    |
           v    v    v
        freeze cpu  pm_ops DFS
        userspc state
                      |
                      v
                 device tree
                 (drivers w/ pm_ops)
```

## S3 suspend sequence

```
trigger: lid close OR `echo mem > /sys/power/state`
  1. pm.freeze_userspace()                  # SIGSTOP-class to all userspace tasks
  2. for dev in dev_tree DFS (pre-order):
        dev.pm_ops.suspend()
        dev.pm_ops.suspend_late()
        dev.pm_ops.suspend_noirq()
  3. cpu_state.save()                        # GSBASE, MSRs, CRx, IDTR/GDTR, XSAVE
  4. ACPI _PTS(3)
  5. write SLP_TYPa(3), SLP_ENa(1) to PM1a_CNT
                  ↓
              hardware S3
                  ↑
  6. wake vector → trampoline → cpu_state.restore()
  7. ACPI _WAK(3)
  8. for dev in dev_tree DFS (reverse-order):
        dev.pm_ops.resume_noirq()
        dev.pm_ops.resume_early()
        dev.pm_ops.resume()
  9. recompute HPET-monotonic delta vs RTC
 10. pm.thaw_userspace()
```

## Hibernate (S4) flow

```
trigger: `echo disk > /sys/power/state`
  pm.freeze_userspace()
  pm_ops.suspend (DFS)
  hibernate::snapshot_ram() ──> swap (M13)
                       │
                       v
              [header, page bitmap, page data, checksum]
  ACPI _PTS(4); SLP_TYPa(4)
                  ↓ power off
                  ↑
  next boot: bootloader detects FACS hibernate signature
            kernel reads image header
            restores pages
            cpu_state.restore()
            pm_ops.resume (DFS reverse)
            pm.thaw_userspace()
```

## Lid/backlight event flow

```
hardware: lid close
  -> ACPI SCI fires
  -> kernel SCI handler reads GPE status
  -> walks `_Lxx`/`_Exx` AML method
  -> _LID method evaluates to current state
  -> pm::events::emit(LID_CLOSE)
       -> /dev/input/event* (compositor consumer)
       -> default policy: trigger S3 if AC unplugged

backlight key:
  ACPI _Q22 / _Q23 -> pm::events::emit(BACKLIGHT_UP)
  -> /sys/class/backlight/intel_backlight/brightness += step
  -> compositor reads sysfs, repaints OSD
```

## cpufreq governor loop

```
1 Hz timer in pm::cpufreq:
  for cpu in online_cpus:
    util = sched.utilization(cpu, last_window)
    target_p = governor.next_pstate(util)
    if target_p != current_p:
      msr_write(MSR_IA32_PERF_CTL, target_p)
      current_p = target_p
```

## Battery + thermal loop

```
5 Hz timer:
  for zone in thermal_zones:
    temp = acpi.eval(zone._TMP)
    sysfs.write(zone, temp)
    if temp >= zone.trip_critical:
        pm.shutdown_emergency()

1 Hz timer:
  for bat in batteries:
    bst = acpi.eval(bat._BST)
    capacity = bst.remaining * 100 / bif.full_charge
    sysfs.write(bat, capacity)
    if capacity < 5 && !on_ac:
        compositor.notify(low_battery)
```

## Failure paths

- **Driver suspend returns Err** → pm core aborts S3, walks DFS in reverse to resume devices already suspended; user sees "suspend failed" notification.
- **Wake from S3 with corrupt CPU state** → kernel oops captured to journal (M16); next boot detects "unsafe resume" and skips hibernate image restore.
- **Hibernate checksum mismatch** → boot ignores image, performs cold boot; user warned via journal.
- **ACPI SCI storm** → rate-limit handler; warn after 1k events / sec.
- **cpufreq MSR write failure** (silicon variant) → fall back to `performance` governor; log once.

## Data structures

```rust
pub trait PmOps: Send + Sync {
    fn suspend(&self) -> Result<(), Errno> { Ok(()) }
    fn suspend_late(&self) -> Result<(), Errno> { Ok(()) }
    fn suspend_noirq(&self) -> Result<(), Errno> { Ok(()) }
    fn resume_noirq(&self) -> Result<(), Errno> { Ok(()) }
    fn resume_early(&self) -> Result<(), Errno> { Ok(()) }
    fn resume(&self) -> Result<(), Errno> { Ok(()) }
}

pub struct Device {
    pub name: &'static str,
    pub parent: Option<DeviceId>,
    pub pm: Option<Arc<dyn PmOps>>,
    pub no_suspend: bool,
}

pub struct CpuState {
    pub gsbase: u64,
    pub kernel_gsbase: u64,
    pub efer: u64, pub star: u64, pub lstar: u64, pub fmask: u64,
    pub cr0: u64, pub cr3: u64, pub cr4: u64,
    pub idtr: DescriptorTablePointer,
    pub gdtr: DescriptorTablePointer,
    pub xsave: [u8; XSAVE_SIZE],
}

pub enum SleepState { S0, S3, S4, S5 }
pub enum PmEvent { LidClose, LidOpen, PowerBtn, BacklightUp, BacklightDown, Battery(u8) }
```
