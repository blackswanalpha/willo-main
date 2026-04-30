# work27 — M27: Backup engine `willo-back` (BLAKE3 CDC, age-encryption, restic-class repo)

> Derived from `docs/idea.md` §20 (Backup, Sync & Recovery).

## Goal

Ship an incremental, encrypted, deduplicated, content-addressed backup engine. The `willo-back` daemon snapshots WilloFS subvolumes (M16), splits into **content-defined chunks** with a Rabin/Buzhash-class polynomial, hashes with **BLAKE3**, encrypts each chunk with **age**, and pushes to local disk, **WebDAV**, or **S3**. A TOML schedule + retention policy controls cadence; CLI is symmetrical for `restore`.

## Depends on

- **M16** — WilloFS snapshots (the read-side source).
- **M14** — netstack + rustls (WebDAV/S3 transport).
- **M19** — package manager (ships `willo-back` as a daemon `.willo`).
- **M37** — user identity (per-user keypair).

## Acceptance criteria

- [ ] `willo-back snapshot` writes a snapshot referencing existing chunks where possible (>90% dedup ratio across hourly snapshots of `~/Documents`).
- [ ] `willo-back restore --snap @latest --dest /tmp/restore` reproduces the source byte-identical.
- [ ] Chunk store is content-addressed; deleting a chunk that is referenced by any snapshot is impossible without `prune`.
- [ ] All three backends (`local`, `webdav`, `s3`) pass the same round-trip test suite.
- [ ] Encryption uses age (X25519); per-user keypair stored via §38 SecretService.
- [ ] Schedule + retention (`hourly:24, daily:7, weekly:4, monthly:12`) respected.
- [ ] `kernel/tests/back_roundtrip.rs` (hosted as a userspace integration) covers create/snapshot/restore.
- [ ] Repo metadata corruption surfaces "repository check failed" instead of silent data loss.

## Task breakdown

### T1. CDC chunker — `userspace/willo-back/chunker.rs`
- Rabin polynomial (single fixed irreducible poly per repo); 1 MiB target, 512 KiB–8 MiB bounds.
- Hash with BLAKE3; output `(content_hash, len, bytes)`.

### T2. Chunk store — `userspace/willo-back/store/`
- `Store` trait with `put(hash, encrypted_bytes)`, `get(hash)`, `has(hash)`, `list_prefix(byte)`, `delete(hash)`.
- Backends: `local` (sharded by first 2 hex chars), `webdav` (via §14 + rustls), `s3` (sigv4 via small custom signer to avoid a heavy SDK).

### T3. Snapshot index — `userspace/willo-back/snapshot.rs`
- One JSON file per snapshot: `{ id, timestamp, parent, tree: { path -> chunk_list } }`.
- Tree blobs themselves chunked + content-addressed.

### T4. age-encryption envelope — `userspace/willo-back/crypt.rs`
- Per-repo key file (X25519); each chunk wrapped in age recipient envelope.
- Optional second recipient (admin recovery key).

### T5. Daemon + scheduler — `userspace/willo-back/willobackd.rs`
- TOML config at `/etc/willo/back.toml` (sources, target, schedule, retention).
- Inotify-class triggers for ad-hoc snapshots on directory change.

### T6. CLI — `userspace/willo-back/cli.rs`
- Subcommands: `snapshot`, `restore`, `ls`, `prune`, `check`, `keys`.

### T7. Repo check + repair — `userspace/willo-back/check.rs`
- Walk all snapshots; verify every referenced chunk exists and decrypts; report missing/corrupt.

### T8. WilloFS snapshot integration — `kernel/src/fs/willofs/snap.rs`
- Reuse M16 snapshot API; willo-back creates a read-only snapshot before scanning.

## New / modified files

| Path | Change |
| --- | --- |
| `userspace/willo-back/*` | **new** |
| `kernel/src/fs/willofs/snap.rs` | + read-side snapshot API |
| `Cargo.toml` (root) | add userspace member |

## Tests to add

- `userspace/willo-back/tests/cdc_stable.rs` — same input → same chunk boundaries across runs.
- `userspace/willo-back/tests/dedup_ratio.rs` — ≥90% dedup on a synthetic hourly workload.
- `userspace/willo-back/tests/back_roundtrip.rs` — backup + restore byte-identical (per backend).
- `userspace/willo-back/tests/check_corrupt.rs` — corrupted chunk detected; clean exit code 2.

## Risks & open questions

- **CDC polynomial drift** — pick once and freeze; changing it breaks dedup forever.
- **Encryption key loss** — without the key, all snapshots are bricked; surface a clear "back up your key" UX prompt at first run.
- **Network speed vs. chunk size** — tune chunk size against §14 throughput; expose advanced setting.
- **S3 vendor diff** — only AWS sigv4 + path-style supported in v1; B2/R2 documented as best-effort.
- **WilloFS snapshot cost** — large snapshot trees may stress M16; coordinate with FS team for tunables.
- **Restore correctness on encryption alg upgrade** — repo metadata pins crypto suite; document a migration tool.
