# flow56 — cross-cutting: suspend / resume across all subsystems

> Cross-cutting flow citing M14, M16, M17, M18, M21, M22, M23, M28, M29, M35, M37, M40, M42, M44, M48.

Picks up the §23 PM core flow and adds the per-subsystem story for everything that has to participate. Use this as the reference for "why did my laptop break on resume?".

## Suspend (entering S3) — DFS pre-order on device tree

```
trigger: lid close OR /sys/power/state=mem
  ┌────────────────────────────────────────────────────────────┐
  │  pm.freeze_userspace                                       │
  │   -> SIGSTOP-class to all userspace tasks                  │
  │   -> userspace daemons receive PM_FREEZE D-Bus signal      │
  │      and persist transient state                           │
  └────────────────────────────────────────────────────────────┘
       │
       v
  ┌────────────────────────────────────────────────────────────┐
  │  per-driver pm_ops::suspend (DFS pre-order)                │
  │   tier 1: leaves                                           │
  │     M22 GPU       -> save engine state, fence drain        │
  │     M40 audio HDA -> mute, save mixer, stop DMA            │
  │     M21 wlan0     -> deauthenticate; preserve PSK in keyring│
  │     M14 NIC eth0  -> link down; stop tx/rx                 │
  │     M42 wg0       -> stop poll; preserve session keys      │
  │     M48 hwmon i801-> quiesce; flush PWM to safe value      │
  │     M43 BT hci0   -> stop scan; disconnect; persist LTKs   │
  │   tier 2: buses                                            │
  │     M13 USB xHCI  -> port suspend; remember per-port state │
  │     M13 PCIe      -> PME enable; D3hot                     │
  │     M13 AHCI/NVMe -> flush; suspend                        │
  │   tier 3: core                                             │
  │     M44 RT runqueues -> drain; release deadlines           │
  │     M16 journal     -> sync; mark "suspended"              │
  │     M29 perf        -> stop sampling                       │
  └────────────────────────────────────────────────────────────┘
       │
       v
  cpu_state.save() (gs, MSRs, CR3, IDTR, GDTR, XSAVE)
  ACPI _PTS(3); SLP_TYPa=3; SLP_ENa=1
       │
       v
   hardware S3
```

## Resume (exit S3) — DFS reverse-order

```
wake: ACPI source (RTC, lid open, USB, WiFi, button)
  -> wake vector → trampoline
  -> cpu_state.restore()
  -> ACPI _WAK(3)
  -> drivers resume DFS reverse:
       core → buses → leaves
  -> recompute HPET-monotonic delta vs RTC
  -> pm.thaw_userspace
  -> userspace daemons receive PM_RESUME D-Bus signal:
       §28 willoc-cloud: revalidate OAuth tokens; re-establish long polls
       §35 willomail-sync: refresh JMAP push; reconnect IMAP IDLE
       §21 wlan0: re-associate; DHCP renew if lease expired
       §42 wg0: send keepalive; verify counters; rekey if stale
       §40 audio: re-init mixer; reload sound theme
       §22 GPU: redraw all surfaces
       §44 RT: re-grant priorities; threadirq priorities reapplied
       §17 compositor: full redraw; cursor reposition
       §48 fancontrol: reread sensors; recompute target PWM
```

## What typically breaks (and how Willo guards against it)

```
issue                                   guard
-----------------------------------------------------------------
Driver missing pm_ops -> bad state      "no-suspend" capability bit; refuse S3
                                        unless all drivers participate (§23)
HPET monotonic skew                     explicit recompute on resume; tested in
                                        kernel/tests/suspend_resume.rs
WiFi 4WH stale                          full re-associate on resume; cached PSK
                                        from keyring (no user prompt)
OAuth refresh stuck                     §28 backoff + UI prompt after 3 fails
GPU mode-set fails                      compositor re-creates surface; falls
                                        back to software KMS path (M22)
USB device change while suspended       hotplug events queued; fired on resume
                                        in DFS order
RT thread priority lost                 §44 willortkit re-grants on PM_RESUME
                                        signal
Bluetooth pair re-discovery             profiles reconnect on advertisement
                                        match against bonded keys (§43)
fancontrol overshoot                    on first sample, clamp transition rate;
                                        emergency full-speed if T > critical
```

## Hibernation (S4) deltas

```
suspend path additionally:
  hibernate::snapshot_ram() -> swap (M13)
       header + page bitmap + page data + checksum
power off
on next boot:
  bootloader detects hibernate signature
  load image; resume normally
  if image checksum fails: cold boot
```

## Userspace contract for daemons

D-Bus signals:

- `org.willo.PM.Freeze` — fired before kernel pm_ops; daemons should sync state.
- `org.willo.PM.Thaw`   — fired after pm_ops resume; daemons should reconnect.

Daemons that take >2 s in Freeze are killed and forced; logged.

## Failure paths

- **Driver suspend Err** → PM core aborts S3; resume DFS-reverse partial; user sees "suspend failed" toast.
- **Wake from S3 with corrupt state** → kernel oops captured (§32); next boot detects "unsafe resume"; rollback to cold boot.
- **Hibernate checksum fail** → cold boot; user warned via journal.
- **OAuth refresh fails on resume** → §28 marks accounts "needs reauth"; UI prompt.
- **WG endpoint moved during sleep** → first inbound packet updates endpoint (§42 roaming).

## Tunables

- `/etc/willo/pm.toml`: lid policy, button policy, idle timeout.
- Per-driver `no-suspend` flag.
- Hibernate enabled or disabled (default disabled until §38 secure-hibernate lands).
- `auto-rt-restore` (§44) on/off.

## Observability

```
journalctl -t pm:
  PM:freeze user=lin
  PM:dev-suspend hci0 ms=3
  PM:dev-suspend xhci_hcd ms=12
  PM:enter-s3
  PM:wake source=lid_open ms=elapsed=12345
  PM:dev-resume xhci_hcd ms=18
  PM:thaw user=lin
willomonitor (§36) shows last suspend/resume duration + per-driver budget.
§29 willotrace pm program collects driver suspend/resume timings.
```

## Test posture

- `kernel/tests/suspend_resume.rs` — basic.
- `kernel/tests/hibernate.rs` — S4.
- `kernel/tests/wifi_resume_assoc.rs` — wlan re-associates.
- `kernel/tests/wg_resume_keepalive.rs` — WG keepalive after wake.
- `kernel/tests/gpu_resume_redraw.rs` — GPU mode-set survives.
