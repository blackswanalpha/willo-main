# flow49 — M49: architecture & runtime flows (Migration & recovery)

> Derived from `docs/idea.md` §20 and the work49 plan.

## Component map

```
        bootloader (§47)
          |
          +-- "Recovery" entry --> willo-recovery.willo-img
                                   (squashfs root + initramfs)
          |
          v
        running recovery system
          |
          +- willo-installer (install/repair primary)
          +- willo-migrate (from Willo / Ubuntu / Windows)
          +- willocrypt   (FDE rebind, regen-binding)
          +- willo-back   (restore)
          +- willofiles, shell, network

        normal system
          |
          +- willo-history (FUSE-overlay over §27 backup)
          +- willoc-history (GUI)
```

## Recovery boot

```
boot menu pick: Recovery
  bootloader: load willo-recovery kernel + initrd from slot or USB
  set "recovery" mode flag (bypass §47 retry counter)
  jump kernel
kernel:
  squashfs root pivot
  init starts:
    bring up display (KMS), keyboard, network (DHCP)
    launch willoc-recovery-launcher
launcher menu:
  - Reinstall Willo
  - Repair primary install (fsck, FDE rebind)
  - Restore from backup (willo-back)
  - Migrate from another OS (willo-migrate)
  - Open shell
```

## willo-migrate from Ubuntu

```
willo-migrate scan
  detect partitions; identify /etc/os-release "Ubuntu"
  list users in /etc/passwd
willo-migrate import lin --from /dev/sda3 --target /home
  mount /dev/sda3 ro at /mnt/src
  /mnt/src/etc/passwd -> map name "lin"; uid 1000 -> Willo new uid
  copy /mnt/src/home/lin -> /home/lin (preserve perms; remap uid)
  parse /mnt/src/var/lib/dpkg/status -> installed packages
  for each: lookup §19 mapping table -> Willo package name
    e.g. firefox -> willo-browser
         code -> willedit (§34) + extension hints
  emit /home/lin/willo-migrate-report.md (mapped vs unmapped)
```

## willo-migrate from Windows

```
willo-migrate scan
  ID NTFS (boot.ini / BCD)
willo-migrate import lin --from /dev/sda4 --target /home
  mount NTFS ro at /mnt/win
  copy Users/Lin/{Documents,Pictures,Music,Videos,Desktop} -> /home/lin/
  inspect Programs Files / AppData / Registry (parsed offline) for installed apps
  produce mapping report
  IMPORTANT: do not write back to NTFS partition
```

## willo-migrate from Willo (peer)

```
willo-migrate import lin --from ssh://lin@otherwillo
  ssh into peer (§37 willosshd)
  remote: tar -czf - /home/lin /etc/passwd-ent /etc/group-ent | ...
  local: receive; remap uid; install
  app list: ssh "willo-pkg list -u" -> install all on local
```

## File-level history (willo-history)

```
mount: /home/$user/.history/  (read-only FUSE)
listdir(.history) -> mirrors normal home
listdir(.history/Documents/note.md) -> [
  "2026-04-30T08:00:00Z (current)",
  "2026-04-29T17:00:00Z",
  "2026-04-28T17:00:00Z (daily)",
  "2026-04-22T17:00:00Z (weekly)",
]
read(.history/Documents/note.md/2026-04-29T17:00:00Z)
  -> §27 willo-back chunks for that snapshot are decrypted on the fly
right-click "Restore this version" in willofiles
  -> willoc-history copies version -> live path
  -> live path's prior content moved to .history-trash
```

## Recovery doesn't trip rollback

```
bootloader detects recovery mode:
  do NOT decrement slot.retry
  do NOT mark slot successful or failed
on exit (reboot from recovery):
  go back to whichever slot was active before
```

## Failure paths

- **Source FS unreadable** → migrate aborts cleanly; partial copies discarded; user notified.
- **App mapping miss** → leaves entry "unmapped"; report surfaces list.
- **Recovery network down** → recovery still works locally; "Reinstall over network" disabled with explanation.
- **History view error** (chunk missing) → entry shown but greyed; user warned to run `willo-back check`.
- **Restore version into protected path** (§39) → falls back to user prompt + audit.

## Data structures

```rust
pub struct RecoveryImage {
    pub kernel: PathBuf,
    pub initramfs: PathBuf,
    pub rootfs: PathBuf,                     // squashfs
    pub version: SmolStr,
    pub channel: Channel,                    // stable | beta | nightly
}

pub struct MigrationSource {
    pub kind: MigrationKind,                 // Willo | Ubuntu | Windows
    pub mount: PathBuf,
    pub users: Vec<MigrationUser>,
    pub apps: Vec<MigrationApp>,
}

pub struct MigrationUser {
    pub name: SmolStr,
    pub home: PathBuf,
    pub uid_src: u32,
    pub shell: PathBuf,
}

pub struct AppMapEntry {
    pub src_id: SmolStr,                     // dpkg name / Windows display name
    pub willo_pkg: Option<SmolStr>,
    pub note: Option<SmolStr>,               // "manual install required"
}

pub struct HistoryEntry {
    pub snapshot_id: SnapshotId,
    pub ts: u64,
    pub label: SmolStr,                      // "current" | "daily" | "weekly" | "monthly"
}
```
