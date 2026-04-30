# flow28 — M28: architecture & runtime flows (Cloud sync + online accounts)

> Derived from `docs/idea.md` §20 and the work28 plan.

## Component map

```
                  +-----------------------+
                  |  user apps (mail,     |
                  |  cal, browser, etc.)  |
                  +----------+------------+
                             | D-Bus
                             v
        +---------------------------------------+
        | willoc-accounts (OAuth tokens, passwd)|
        +-------------------+-------------------+
                            |
                            v (creds)
        +---------------------------------------+
        | willoc-cloud daemon                   |
        |  +------+  +-------+  +-------+       |
        |  |watch |  |sync   |  |crdt   |       |
        |  +---+--+  +---+---+  +---+---+       |
        +------+--------+----------+------------+
               |        |          |
               v        v          v
        VFS FUSE-hook  HTTPS    WebSocket
        ~/Cloud/...   backends  CRDT peers
```

## Mount layout

```
~/Cloud/
   ├── dropbox/         (CloudBackend = Dropbox)
   ├── onedrive/        (CloudBackend = OneDrive)
   ├── webdav-work/     (CloudBackend = WebDAV, account "work")
   ├── s3-myorg/        (CloudBackend = S3, bucket "myorg")
   └── shared/          (CRDT-backed shared docs)
```

## Local edit → remote upload

```
app writes ~/Cloud/dropbox/notes.txt
  -> VFS write ok
  -> watch::on_change(path)
       -> debounce 200ms
       -> sync::queue_upload(path)
sync worker:
  -> backend::put(remote_path, bytes, etag=current)
       -> HTTPS PUT (rustls + §14)
       -> 200 OK; etag updated locally
journal logs "upload: dropbox: notes.txt 1.2 KiB ok"
```

## Remote edit → local update

```
sync worker (per backend):
  loop:
     etag = backend.poll(prefix=/, since=last_token)
     if change:
        for path in changed:
           local_etag = stat(path).etag
           if local_etag == prior_remote_etag:
              # no local divergence → just download
              bytes = backend.get(path)
              vfs::write(path, bytes); update etag
           else:
              conflict::resolve(path)
```

## Conflict resolution (file-level)

```
both local and remote changed since last sync token
  -> backend.get(path) -> remote_bytes
  -> rename local "{name} (conflict-{ts}).{ext}"
  -> write remote_bytes as canonical
  -> emit notification: "Conflict in notes.txt; both versions kept"
```

## CRDT-backed shared doc (Automerge)

```
two devices A, B editing ~/Cloud/shared/plan.md
  A: edit -> local Automerge::change -> changes pushed to backend (changes-A.bin)
  B: long-poll receives changes-A.bin
     B: Automerge::apply_changes -> doc bytes regenerated
     B: edit -> local change -> push changes-B.bin
  A: receives changes-B.bin
     A: apply -> deterministic merge (no conflicts)
periodic compaction:
  if doc.history > 1000 ops: snapshot + truncate history; bump epoch
```

## OAuth refresh across §23 sleep

```
access_token expired or near expiry
  -> request_refresh(refresh_token)
  -> POST /token -> new access_token + (sometimes) new refresh_token
  -> persist via §38 SecretService
on §23 resume:
  willoc-cloud receives "PM_RESUME" event
  -> for each backend: revalidate token; if 401, refresh; if refresh fails, mark "needs reauth"
```

## Failure paths

- **Backend 401** → mark account "needs reauth"; UI prompts; no further uploads attempted.
- **Network unavailable** → queue grows in `~/Cloud/.willo/queue/`; flushed on reconnect.
- **Disk full on local cache** → daemon stops downloads; surfaces "low disk" notification.
- **CRDT history corruption** → fall back to last known snapshot; UI warns "history truncated".
- **Conflicting renames** → keep both with `(conflict-{ts})` suffix; never silently drop.

## Data structures

```rust
pub trait CloudBackend: Send + Sync {
    async fn list(&self, path: &Path) -> Result<Vec<RemoteEntry>>;
    async fn stat(&self, path: &Path) -> Result<RemoteEntry>;
    async fn get(&self, path: &Path) -> Result<Vec<u8>>;
    async fn put(&self, path: &Path, bytes: &[u8], if_match: Option<&str>) -> Result<String>;
    async fn delete(&self, path: &Path) -> Result<()>;
    async fn watch(&self, since: Option<&str>) -> Result<(Vec<RemoteEntry>, String)>;
}

pub struct RemoteEntry {
    pub path: PathBuf,
    pub size: u64,
    pub mtime: u64,
    pub etag: String,
    pub kind: EntryKind,                 // File | Dir
}

pub struct Account {
    pub id: AccountId,
    pub backend: BackendKind,
    pub access_token: SecretString,      // never logged
    pub refresh_token: SecretString,
    pub expires_at: u64,
    pub scopes: Vec<String>,
}

pub struct CrdtDoc {
    pub id: DocId,
    pub automerge_state: automerge::AutoCommit,
    pub last_remote_heads: Vec<automerge::ChangeHash>,
}
```
