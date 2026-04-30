# work19 — M19: Package manager, repo, signed updates

> Derived from `docs/idea.md` §15 (Package Management & Software Ecosystem).

## Goal

Ship a complete software-distribution pipeline: a `.willo` package format, a content-addressed repository protocol with signature verification, a SAT-based dependency resolver, and an atomic A/B updater so the system can update kernel + userspace and roll back on a bad boot.

## Depends on

- **M16** — WilloFS snapshots (used as the rollback mechanism).
- **M14** — networking (fetch from repo).
- **M12** — services (update daemon).

## Acceptance criteria

- [ ] `.willo` package format defined; spec lives at `docs/spec/package.md` (out of scope for this doc).
- [ ] `willo-pkg build` produces a signed `.willo` from a manifest + tree.
- [ ] `willo-pkg install <name>` resolves deps, fetches, verifies signatures, installs atomically.
- [ ] `willo-pkg upgrade` swaps the running root via WilloFS snapshot + reboot.
- [ ] Failed boot (3 attempts) auto-rolls back to previous snapshot (counter in firmware-vars or boot-sector).
- [ ] Repo metadata signed; key trust store at `/etc/willo-pkg/keys/`.
- [ ] First-party software center GUI (`willoc-center`) browses + installs.

## Task breakdown

### T1. Package format — `userspace/willo-pkg/format.rs`
- `manifest.toml` (name, version, deps, files, hooks).
- Tar-like file table with per-file SHA-256.
- `.sig` blob: ed25519 over the manifest+tree-hash.

### T2. Repo protocol — `userspace/willo-pkg/repo.rs`
- HTTPS GET; signed `index.json` lists `(name, version, url, sha256, sig)`.
- Mirrors via simple round-robin.

### T3. Dep resolver — `userspace/willo-pkg/resolve.rs`
- SAT/PB solver (port a small one such as `pubgrub`/`libsolv`-class).
- Conflict messages mention the user-friendly cause.

### T4. Installer — `userspace/willo-pkg/install.rs`
- Stage tree under `/.staging/<txn>/`.
- Run pre-install hook in a sandbox.
- WilloFS snapshot + atomic rename swap to live tree.
- Run post-install hook.
- Update manifest db at `/var/lib/willo-pkg/`.

### T5. Update daemon — `userspace/willo-updd/`
- Periodic check (configurable).
- A/B kernel slots: write the new kernel to the inactive slot, set `boot_next`, reboot.
- Rollback: bootloader counts boot attempts; ≥3 fails → revert `boot_next`.

### T6. Signing tooling — `userspace/willo-sign/`
- Key generation, manifest signing, key rotation, revocation list.

### T7. Software center — `userspace/willoc-center/`
- Compositor client.
- Browse repo categories, search, install/remove, show progress.

### T8. Bootloader hooks
- Read/write `boot_next`, `boot_count`, slot pointers from EFI variables (UEFI path) or a known sector (BIOS path).
- Surfaced as `/sys/firmware/willo-boot/`.

## New / modified files

| Path | Change |
| --- | --- |
| `userspace/willo-pkg/` (CLI + libs) | **new** |
| `userspace/willo-updd/` | **new** |
| `userspace/willo-sign/` | **new** |
| `userspace/willoc-center/` | **new** |
| `kernel/src/firmware/willo_boot.rs` | **new** (slot vars surface) |
| `bootloader/` | hooks for slot selection + boot count |
| `/var/lib/willo-pkg/` | runtime DB layout |

## Tests to add

- `kernel/tests/firmware_slot.rs` — slot vars round-trip.
- Userspace integration:
  - `willo-pkg build` → install → uninstall round-trip.
  - Tampered package → install rejected.
  - Missing dep → resolver error message includes the missing name.
  - `willo-updd` simulates an update: snapshot, boot fails 3×, system rolls back.

## Risks & open questions

- **Resolver complexity** — port an existing solver before writing one.
- **Hook sandboxing** — leans on §12 (Security) sandbox primitives; until those land, hooks run as root with `seccomp`-style allowlist.
- **Bootloader↔kernel slot ABI** — small, but irreversible once shipped; design the bytes carefully.
- **Atomic swap on FAT-rooted systems** — we standardise on WilloFS root; FAT-rooted is unsupported for upgrades.
- **GUI scope** — center is read-mostly first; reviews/ratings/dependencies-graph deferred.
