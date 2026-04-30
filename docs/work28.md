# work28 — M28: Cloud sync + online accounts (~/Cloud/, rclone-class backends, CRDT for shared docs)

> Derived from `docs/idea.md` §20 (Backup, Sync & Recovery).

## Goal

Make `~/Cloud/` a first-class part of the file system. A mount-style integration exposes per-backend subdirs (`~/Cloud/dropbox/`, `~/Cloud/onedrive/`, `~/Cloud/webdav/`); a `willoc-cloud` daemon syncs both directions and handles conflicts. Shared documents (`~/Cloud/shared/*.md` and similar) sync via **Automerge** CRDT for conflict-free collaboration. A single **online-accounts** service stores OAuth tokens + app passwords, feeding both this sync and §35 mail/calendar.

## Depends on

- **M14** — netstack + rustls.
- **M16** — VFS hooks for FUSE-style mount.
- **M27** — backup engine (shares the chunk-store crypto patterns).
- **M37** — user identity (per-user account store).

## Acceptance criteria

- [ ] `~/Cloud/<backend>/` directories exist after `willoc-cloud` starts; each backend mounts as a virtual FS.
- [ ] WebDAV + S3 + Dropbox + OneDrive backends round-trip a 10 MB file.
- [ ] OAuth token refresh handles token expiry transparently across §23 sleep cycles.
- [ ] Local edit → remote upload within 5 s of write completion.
- [ ] Remote edit → local update within 30 s of long-poll trigger.
- [ ] Two-device edit on the same Markdown CRDT-doc converges with no conflict marker.
- [ ] Accounts service exposes a D-Bus API; `willomail`/`willocal` consume it for credentials.
- [ ] `kernel/tests/cloud_roundtrip.rs` (userspace integration) round-trips local↔remote across all in-tree backends.

## Task breakdown

### T1. Online accounts service — `userspace/willoc-accounts/`
- D-Bus daemon at `org.willo.OnlineAccounts`.
- Storage: encrypted at rest via §38 SecretService; per-user key.
- OAuth flows (PKCE) for Dropbox/OneDrive/Google; password storage for WebDAV/IMAP.

### T2. Backend trait — `userspace/willoc-cloud/backends/mod.rs`
- `trait CloudBackend { list, stat, get, put, delete, watch, link_share }`.
- Authentication via `willoc-accounts` D-Bus.

### T3. Concrete backends — `userspace/willoc-cloud/backends/{webdav,s3,dropbox,onedrive,sftp}.rs`
- HTTP via §14 + rustls; small, focused REST clients (no monolithic SDKs).
- `s3` shares sigv4 signer with M27 for code reuse.

### T4. FUSE-style mount — `userspace/willoc-cloud/mount.rs`
- VFS hook (M16 FUSE-style) to expose `~/Cloud/<backend>/` as a mount.
- Read paths fetch on demand; write paths queue upload.

### T5. Local watcher — `userspace/willoc-cloud/watch.rs`
- inotify-class events from VFS; debounce 200 ms; queue uploads.

### T6. Conflict resolution — `userspace/willoc-cloud/conflict.rs`
- File-level: `name (conflict-{ts}).ext` keep-both UI prompt.
- Document-level: hand to CRDT layer.

### T7. CRDT layer — `userspace/willoc-cloud/crdt.rs`
- Automerge-rs for shared `.md`/`.json` docs.
- Encoded change log committed alongside doc bytes.
- Sync protocol over WebSocket (§14) when both peers online; backend storage between.

### T8. Settings UI — `userspace/willoc-cloud/ui.rs`
- Compositor client to add/remove accounts and view sync status.

## New / modified files

| Path | Change |
| --- | --- |
| `userspace/willoc-accounts/` | **new** |
| `userspace/willoc-cloud/` | **new** |
| `kernel/src/fs/fuse/` | + small additions for mount handles |

## Tests to add

- `userspace/willoc-cloud/tests/backend_roundtrip.rs` — per-backend put/get/delete.
- `userspace/willoc-cloud/tests/oauth_refresh.rs` — token expiry handled.
- `userspace/willoc-cloud/tests/crdt_converge.rs` — two writers, divergent edits, converge.
- `userspace/willoc-accounts/tests/secret_at_rest.rs` — token bytes encrypted before disk write.
- `kernel/tests/cloud_roundtrip.rs` — boot + cloud daemon + backend put/get round-trip.

## Risks & open questions

- **OAuth token leak** — must transit kernel/userspace boundary only via §38 SecretService; never log to journal.
- **Backend rate limits** — exponential backoff per backend; surface in UI.
- **CRDT memory blow-up** — Automerge change log can grow unbounded; periodic compaction with snapshot baseline.
- **Sleep/wake** — long-polls die during S3; resume hook (§23) re-establishes connections.
- **Offline writes** — must be idempotent on reconnect; rely on per-file `etag` for conditional `If-Match` PUT.
- **Sandboxing** — `willoc-cloud` needs broad network + file scopes; tight §39 profile but cannot be too restrictive.
