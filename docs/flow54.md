# flow54 — cross-cutting: crash → minidump → recovery → bug report

> Cross-cutting flow citing M11, M16, M19, M27, M32, M36, M47, M49.

What happens between a crash (kernel oops or userspace SIGSEGV) and the user being back to a working system, with a symbolicated, optionally-uploaded crash report.

## Userspace SIGSEGV path

```
process X dereferences null
  -> CPU #PF
  -> kernel handler: not handled in kernel; deliver SIGSEGV
  -> if process has SIGSEGV handler: run it (rare; usually default)
  -> default: §32 coredump::write_minidump
        capture: regs, threads, stack(s) ≤ 256 KiB/thread, VMA map, modules+build_ids, signal info
        write to /var/crash/{boot_id}-{pid}-{ts}.mdmp
        notify §36 willoc-crash via D-Bus
  -> kernel kills X
  -> X's parent reaps via wait4
```

## Kernel oops path

```
kernel panic or BUG_ON
  -> halt all CPUs (IPIs); §32 halt_all
  -> dump_stack() to serial AND framebuffer
  -> §16 journal::write_emergency() (in-memory; flushed on next boot if disk lost)
  -> §32 coredump::write_kernel_minidump:
        regs (per-CPU snapshot), kernel stack, modules with build_ids
        write to a known LBA OR /var/crash/kernel-{boot_id}.mdmp via emergency disk path
  -> per /etc/willo/oops.toml: halt | reboot | kdump
```

## Symbolication (§32)

```
willo-symbolicate /var/crash/X.mdmp
  for each module entry:
    bid = build_id
    debug = /usr/lib/debug/.build-id/{bid[0:2]}/{bid[2:]}.debug
    if absent (debuginfo not installed):
       suggest `willo-pkg install <pkg>-dbg`
    load DWARF
  for each frame:
    resolve module + offset → file:line:fn
  emit JSON + pretty text under /var/crash/X.mdmp.symbols
```

## willoc-crash UI (§36)

```
notification: "Application X crashed"
clicking opens willoc-crash:
  list crashes; symbolicated; offer:
    - View report (text + frames)
    - Open ticket (network, opt-in)
    - Delete report
ticket flow:
  user reviews payload (no PII visible by inspection)
  signs with user-key (§38)
  HTTPS POST to vendor URL (or local Bugzilla-class)
  receives ticket id; persists locally
```

## Restart strategies

```
crashed system service:
  systemd-class user manager Restart=on-failure
  exponential backoff (1s, 2s, 5s, 15s)
  after N restarts: leave alone, surface "service stopped"
crashed user app:
  no auto-restart (apps choose their own)
  willoc-shell may suggest reopen
crashed kernel:
  per oops.toml policy (halt for dev; reboot for prod)
  if §47 retry budget exhausted: rollback engages on next boot
```

## Recovery paths (§49)

```
boot loops 3x:
  §47 rollback to previous slot
boot loops 3x on fallback too:
  bootloader stops; user sees Recovery prompt
recovery mode (§49):
  shell tools: fsck (§16), willocrypt repair (§38), willo-pkg --reinstall, willo-back restore
  optionally: willo-installer for full reinstall preserving /home
```

## Persistent observability

```
each crash adds an entry to §16 journal:
  type=crash, kind=user|kernel, pid, exe, signal, ts, mdmp_path
each ticket records ticket_id linking to mdmp
user can query: `journalctl -t crash --since 7d`
```

## Privacy posture

- Coredumps may contain secrets (env, memory contents).
- Storage: `/var/crash/` permissioned 0700 root.
- Upload is opt-in **per crash**, never blanket.
- Report previewer warns about high-entropy strings (potential keys).
- §39 audit logs which user opened/uploaded which report.

## Performance impact

- Minidump capture is fast (≤50 ms typical) because stacks are bounded.
- Symbolication can be expensive (DWARF parse): runs lazily on first view.
- `/var/crash` retention policy (e.g., keep last 50 reports, prune by age).

## Failure paths

- **Disk full** → minidump truncated; header marks "incomplete"; user warned.
- **No build-id** (manually compiled binary) → frames unsymbolicated; user prompted to install symbols.
- **Network offline** → ticket queued; sent on reconnect (§28 wakeups).
- **Crash inside the crash handler** → kernel falls back to bare serial dump + halt.
- **DWARF mismatch** → "<no symbol>"; warn user.

## Tunables

- `/etc/willo/oops.toml`: halt vs reboot, kdump enable.
- `/etc/willo/crash.toml`: max stored reports, max stack bytes, auto-prune age.
- Per-app `crash.toml` opt-out.
- §44 RT mode users may want kernel oops to halt unconditionally (audio safety).

## End state

After a crash:

- Working system (or recovery if rollback engaged).
- Symbolicated minidump available locally.
- Optional ticket filed.
- §16 journal carries the audit trail.
- §27 backup may have a pre-crash snapshot to restore from.
