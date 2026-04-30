# work49 — M49: Migration & recovery (recovery image, migration assistant, file-level history)

> Derived from `docs/idea.md` §20 (Backup, Sync & Recovery — recovery environment, migration assistant, file-level history).

## Goal

Make data loss survivable and platform moves painless. Build a self-contained **recovery environment** image (`willo-recovery`, à la WinRE / macOS Recovery / Ubuntu live), a **migration assistant** that imports settings/files/app-list from another Willo box, an Ubuntu partition, or a Windows install, and a **file-level history** layer that lets any file be rewound from a list of past versions (Time Machine-class), built on top of §27 backup snapshots.

## Depends on

- **M19** — package repository (recovery env consumes it).
- **M27** — backup engine (file history is a derived view).
- **M37** — multi-user (migrate per-user data).
- **M47** — boot menu (Recovery is a menu entry).

## Acceptance criteria

- [ ] `willo-recovery.willo-img` boots from local disk slot (§47) or USB; provides a minimal Willo with shell, networking, fsck, willocrypt, willo-pkg, willo-back.
- [ ] Recovery passes the §47 boot path without disturbing rollback counters.
- [ ] `willo-migrate` imports a user from another Willo, an Ubuntu/Debian partition (passwd + home + apt list), or a Windows install (User profile + Programs list translated to nearest `.willo` packages).
- [ ] File-level history: per-user `~/.history/<path>` virtual view shows file versions back N retention windows; restore is one-click.
- [ ] `kernel/tests/recovery_boot.rs` boots into recovery target.
- [ ] `userspace/willo-migrate/tests/import_ubuntu.rs` migrates a fixture rootfs.
- [ ] `userspace/willo-history/tests/file_versions.rs` shows expected versions.

## Task breakdown

### T1. willo-recovery image — `tools/recovery-image/`
- Build script produces `willo-recovery.willo-img` (squashfs root + initramfs).
- Includes: `bash` (or Willo nushell), `willo-pkg`, `willo-back`, `willocrypt`, `willofiles`, basic drivers (§13/§22/§23/§42).

### T2. Boot menu integration — `bootloader/menu.rs` (extends §47)
- "Recovery" entry boots the recovery image; bypasses A/B retry counters.
- Network mode: optional fetch from `https://recovery.willo.example/<channel>/willo-recovery.willo-img`.

### T3. willo-migrate from Willo — `userspace/willo-migrate/from_willo.rs`
- Source: another Willo box reachable via SSH (§37) or a backup repo (§27).
- Target: copy `/home/$user`, `/etc/passwd|group|shadow`, app list (`willo-pkg list -u`).

### T4. willo-migrate from Ubuntu — `userspace/willo-migrate/from_ubuntu.rs`
- Mount source ext4; map UID/GID; copy `/home/$user`; translate dotfiles where format differs.
- App list: parse `dpkg --get-selections`; suggest Willo equivalents from §19 repo.

### T5. willo-migrate from Windows — `userspace/willo-migrate/from_windows.rs`
- Mount NTFS read-only.
- Copy `Users/<name>/{Documents,Pictures,Music,Videos}` to `/home/$user/`.
- Map known apps (Chrome, Firefox, VS Code, Steam) to Willo equivalents.

### T6. File-level history — `userspace/willo-history/`
- FUSE-style mount: `~/.history/path/to/file/` shows versions sourced from §27 backups.
- Restore: copy chosen version back; original moved to `~/.history-trash/`.

### T7. UI integration — `userspace/willoc-history/`, `userspace/willoc-recovery-launcher/`
- Compositor clients in §36 settings.

### T8. Recovery-mode bootstrap of installer — `userspace/willo-installer/`
- From recovery, install or repair a primary Willo install.

## New / modified files

| Path | Change |
| --- | --- |
| `tools/recovery-image/` | **new** build pipeline |
| `bootloader/menu.rs` | recovery entry exempt from rollback counters |
| `userspace/willo-migrate/` | **new** |
| `userspace/willo-history/` | **new** |
| `userspace/willoc-history/` | **new** GUI |
| `userspace/willo-installer/` | **new** |

## Tests to add

- `kernel/tests/recovery_boot.rs` — recovery image boots; basic tools work.
- `userspace/willo-migrate/tests/import_ubuntu.rs` — fixture ext4 → Willo home.
- `userspace/willo-migrate/tests/import_windows.rs` — fixture NTFS → Willo home (subset).
- `userspace/willo-history/tests/file_versions.rs` — versions enumerated.

## Risks & open questions

- **NTFS read-only** — write-back is risky; v1 strictly read-only; document.
- **Windows app mapping** — best-effort; surface unmapped items to user.
- **Recovery image size** — must fit a small partition; cap at 512 MiB.
- **Privacy** — migration runs as root; clearly warn user; never upload anything.
- **Filesystem for `.history`** — MUST not be writable; FUSE-style overlay enforces.
- **Network recovery** — optional; pin a Willo signing root; avoid user-supplied URLs.
