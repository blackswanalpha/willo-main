# flow53 — cross-cutting: package install + verify + sandbox rollout

> Cross-cutting flow tying M19, M27, M38, M39, M45, M47.

End-to-end picture of what happens when a user installs a package — including signature verification, dependency resolution, sandbox profile rollout, kernel module loading, and (if applicable) A/B kernel slot mechanics.

## Sequence (interactive package install)

```
willoc-center "Install": willo-browser
  -> willo-pkg install willo-browser
       1. resolve deps (SAT solver, M19)
       2. fetch metadata + signed index.json
       3. verify index signature with §38 trust root
       4. for each package in topological order:
            a. download .willo (HTTPS via M14 + rustls)
            b. verify sha256 + ed25519 sig
            c. stage to /.staging/<txn>/<pkg>/
            d. run pre-install hook in sandbox (§39 minimal profile)
            e. WilloFS snapshot (§16) of affected paths
            f. atomic rename to live tree
            g. install §39 MAC profile (e.g., willo-browser.toml)
            h. if module: depmod re-run (M45)
            i. run post-install hook in sandbox
            j. update /var/lib/willo-pkg/db
       5. if any kernel.ko/.wko changed:
            -> route through §47 A/B slot if kernel itself
            -> else: optional `willomod load` for hot-pluggable
       6. emit "installed" event; UI updates
```

## Trust chain

```
trust root: /etc/willo/trust/  (rooted in §38)
  pubkey-system.pem      (Willo system signer)
  pubkey-vendor-X.pem    (third-party vendors)
  index-revocation.json  (expired/revoked keys)

each .willo:
  manifest.toml       (name, version, deps, hooks, module-list)
  files/...
  manifest.sig        (ed25519 over sha256(manifest+files-tree))

§19 verifier:
  sig OK against any trusted pubkey?
  pubkey not in revocation list?
  manifest.version > installed.version (no downgrade unless --allow-downgrade)?
```

## Dependency resolution (SAT)

```
solver input:
  installed: { willo-pkg=1.2, ... }
  available: { willo-browser=125, qt-willo=6.7, ... }
  request: install willo-browser=latest
solver:
  PubGrub-class incremental search
  emit "would install/upgrade/keep" plan
  emit conflict messages with user-friendly cause if unsat
user confirms; install proceeds
```

## Sandbox profile rollout (§39)

```
each new daemon ships with /etc/willomac/<name>.toml
on §19 install:
  validate profile syntax
  if name conflicts with existing profile: error
  install file
  if daemon currently running: SIGHUP or restart to apply
on uninstall:
  remove profile
  any §39 audit denials reference removed profile (still in journal)
```

## Kernel module install (M45)

```
package contains my_drv-1.0.wko targeting kernel 1.0
unpack to /lib/willo-modules/1.0/
willodepmod refreshes /lib/willo-modules/1.0/modules.dep
on hotplug match: willomod load my_drv
verify .willo.sig (same trust root)
load + run module_init
```

## Kernel image install (M47 hot path)

```
package "willo-kernel-1.1" replaces /boot
  -> willo-pkg detects "kind = kernel"
  -> hand to §47 willo-updd:
       write to inactive slot
       update slot_meta: priority, retry=3, successful=0, boot_next=inactive
       reboot (or schedule)
  -> on next boot, §47 logic takes over
```

## Atomic swap (M16)

```
WilloFS snapshot before swap:
  willofs::snapshot("/", "before-pkg-{txn}")
swap:
  for each file:
    rename(/.staging/<pkg>/path, /path)
on success:
  drop snapshot OR keep per retention policy
on failure:
  willofs::rollback(snapshot)
  return Err to user
```

## Backup interplay (§27)

```
willo-back daemon (if active) sees post-install events
  -> automatic snapshot if configured (some users want pre-install + post-install pair)
  -> file-history (§49) shows pre-update state for any modified config
```

## Failure paths

- **Bad signature** → install aborts; nothing staged; clear error.
- **Hook script fails** (post-install non-zero) → snapshot rollback; install reported failed.
- **Disk full** → staging fails early; no permanent state.
- **Module signature ok but vermagic mismatch** → install ok, load deferred; surface "rebuild needed".
- **Kernel install but bootloader can't write** (EFI vars exhausted) → §47 falls back to BIOS sector path; warn.
- **Conflict with running container** (file under bind-mount) → install fails; user must stop container.

## Audit trail (§39)

Every install/uninstall logs to §16 journal as a `willopkg.tx` event:

```json
{
  "ts": 1714530000,
  "event": "willopkg.tx",
  "pkg": "willo-browser",
  "version_old": "120",
  "version_new": "125",
  "user": "lin",
  "trust": "vendor-X",
  "snapshot_before": "before-pkg-3a91",
  "snapshot_after": "post-pkg-3a91",
  "hooks_passed": true
}
```

## Rollback after install

```
user: willo-pkg rollback willo-browser
  -> willofs::rollback(snapshot=before-pkg-3a91 for paths owned by willo-browser)
  -> /var/lib/willo-pkg/db restored
  -> §39 profile restored
  -> if kernel: §47 set boot_next=other slot; reboot
```

## Tunables

- Repository priorities + mirrors (`/etc/willo/repos.d/`).
- Auto-update schedule (§19 willo-updd).
- Pre-install hook sandbox: per-pkg overrides allowed only by signed metadata.
- Snapshot retention: how many pre-install snapshots to keep.
