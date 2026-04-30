# flow60 — cross-cutting: A/B update + rollback + post-update reconciliation

> Cross-cutting flow citing M16, M19, M27, M38, M47, M49.

The full picture for a system update — fetching artefacts, signing, A/B slot writes, measured-boot interplay, post-boot success criteria, rollback semantics, and TPM rebinding.

## Sequence (auto-applied update)

```
T+0          willo-updd timer fires (or manual `willo-updd check`)
T+~          fetch /index.json from repo (HTTPS via §14)
                verify signature against §38 trust root
                compare versions vs /var/lib/willo-pkg/db
                identify candidate update set
T+~          §27 (optional) snapshot current home + system
T+~          download artefacts (kernel, system, packages)
                each verified separately
T+~          stage to inactive slot (§47)
                kernel image: write to /boot/slot-b
                system FS bytes: bind into slot-b WilloFS
T+~          run §47 vbmeta verifier offline (signature + rollback indexes)
T+~          willobootctl prepare:
                slot_b.priority = max+1
                slot_b.retry = 3
                slot_b.successful = 0
                boot_next = b
T+~          (if not silent) prompt user "Restart now? [Restart] [Later]"
T+~          on restart:
                §47 boot menu picks B (because boot_next = b)
                §38 may need to fall back to passphrase if PCRs changed
                                 (reseal happens after first successful login)
                normal boot path (flow51)
T+post-boot  desktop ready
T+30s        §47 willobootctl mark-good:
                slot_b.successful = 1
                §38 willocrypt regen-binding (re-seal LUKS to current PCRs)
                §27 may take post-update snapshot
T+later     repository metadata updated; UI shows "you are up to date"
```

## Failure / rollback (3 failed boots)

```
boot 1 (slot B): retry=3->2; oops
boot 2 (slot B): retry=2->1; oops
boot 3 (slot B): retry=1->0; oops
boot 4: bootloader sees retry=0 && successful=0
   -> swap active to A
   -> mark slot B as "failed-reverted"
   -> boot A
   -> §16 journal gains rollback event
   -> §36 settings shows "Update was rolled back; click for details"
   -> §38 LUKS rebinding deferred until success
post-rollback admin action:
   - investigate journal
   - file ticket (§32 + §54)
   - retry update later
```

## TPM PCR rebinding interplay (§38)

```
PCR-bound LUKS unsealing keys to {PCR 0, 2, 4, 7}
update changes:
  - kernel image (PCR 8 changes; not in seal policy by default → no impact)
  - bootloader (PCR 4 changes → sealing fails)
  - vbmeta keys rotated (PCR 7 changes → sealing fails)
flow:
  on first boot post-update: TPM unseal fails
     fallback: passphrase prompt
     successful login -> willocrypt regen-binding
                          re-seal master key to current PCR set
                          subsequent boots: auto-unlock
```

## Per-package update (no kernel)

```
non-kernel package update (e.g. willo-browser bump):
  -> stage + verify
  -> WilloFS snapshot
  -> atomic rename swap
  -> kill or HUP the running daemon to pick up new binary
  -> NO reboot needed
  -> NO §47 slot machinery touched
  -> §39 audit logged
```

## Recovery escape hatch (§49)

```
both A and B fail (very rare):
  bootloader stops at "all slots failed" UI
  user picks "Recovery" -> §49 willo-recovery
  recovery shell:
     willo-pkg --slot=a force-reinstall  (via journaled package db)
     OR
     willo-back restore --snap=pre-update --target=/
  reboot; normal flow resumes
```

## Deferred / paused updates

```
user picks "Later" prompt:
  artefacts kept staged; mark "ready-pending-restart"
  next reboot prompts again OR auto-applies if user.preference.auto-restart
  if disk pressure: auto-discard stale staging after 7 days; re-fetch later
```

## Backup interplay (§27)

```
willo-updd policy:
  if §27 is configured:
     pre-update snapshot tag = "pre-update-v{old}->v{new}"
     post-update snapshot tag = "post-update-v{new}"
     retention: keep these specially, never auto-prune
this means file-history (§49) shows clear pre/post markers.
```

## Update channels

```
/etc/willo/repos.d/*.toml:
  [stable]    -> only stable releases
  [beta]      -> beta + stable
  [nightly]   -> daily builds (warning at install time)
each channel pinned to a §38 trust subset.
willo-pkg switch-channel beta -> warning + dpkg-style downgrade dance.
```

## Per-app update strategies

```
"hot-swap" daemons (most): reload mid-flight
"restart" daemons (compositor, audio): user-visible blip on update
"reboot" required: kernel, init, libc-shim, glibc; tracked by package "needs_reboot=true" flag
willo-updd batches "needs_reboot=true" updates and prompts once.
```

## Audit + journal

```
each update transaction logs:
  willopkg.tx { pkg, version_old, version_new, snapshot_before, snapshot_after, hooks_passed }
  pm.update_pending if reboot needed
  pm.update_applied { slot, version }
§32 minidump pipeline catches any crash mid-update; report links to txn id.
```

## What "update success" requires

1. Compositor running for ≥30 s.
2. No kernel oops in §16 journal between boot and mark-good.
3. Network reachable (curl https://repo OK).
4. Audio mixer present.
5. PAM session opened by ≥1 user.

If any criterion fails, mark-good is not issued; on third boot, §47 rolls back.

## Failure paths

- **Mid-download crash** → staged artefacts hashed; rejected on retry; clean.
- **Disk full mid-stage** → stage aborted; nothing committed.
- **Bad sig on artefact** → entire update aborted; trust root never expanded silently.
- **Hook script bug** (post-install Err) → snapshot rollback; install reported fail.
- **Bootloader EFI vars exhausted** → §47 falls back to BIOS sector path; warn.
- **PCR rebind fails** post-update → passphrase remains valid; retry rebind on every login until success.

## Tunables

- `/etc/willo/updd.toml`: schedule, channel, auto-restart policy, low-bandwidth mode.
- Per-package: `needs_reboot` flag.
- §27 policy: pre/post-update snapshot retention.
- §38 PCR set: which PCRs participate in seal policy.
- §47 retry budget: default 3.
