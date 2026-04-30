# flow27 — M27: architecture & runtime flows (Backup engine)

> Derived from `docs/idea.md` §20 and the work27 plan.

## Component map

```
        +------------------+
        | /etc/willo/back  |
        +--------+---------+
                 |
                 v
        +------------------+      +-----------------+
        | willobackd       |<-----| §38 SecretSvc   | (age key)
        +---+----------+---+      +-----------------+
            |          |
            v          v
        +-------+   +----------+
        |WilloFS|   |  CDC     |
        |snapshot   |  chunker |
        +-------+   +----+-----+
                        |
                        v
                +----------------+
                |  age-encrypt   |
                +-------+--------+
                        |
                        v
                +-------+--------+
                | Store trait    |
                +-+--+----+---+--+
                  |  |    |   |
                  v  v    v   v
              local webdav s3 (future cloud backends)
```

## Snapshot flow

```
1. timer fires (e.g. hourly)
2. willofs::create_snapshot("/", "back-tmp-{ts}")
3. for entry in walk("/back-tmp-{ts}"):
     read file in 4 MiB pages
     stream into CDC chunker
     for each (hash, len, bytes):
        if !store.has(hash):
            ciphertext = age::encrypt(bytes, recipient)
            store.put(hash, ciphertext)
        record (path, [hashes]) in tree blob
4. tree blob is itself chunked + stored
5. snapshot index { id, ts, parent, tree_root_hash } written
6. willofs::drop_snapshot("/back-tmp-{ts}")
```

## Restore flow

```
1. willo-back restore --snap @latest --dest /tmp/restore
2. fetch snapshot index by id
3. fetch tree blob chunks by hash; decrypt; assemble JSON
4. for each (path, [hashes]) in tree:
     for h in hashes:
         ciphertext = store.get(h)
         plaintext = age::decrypt(ciphertext, identity)
         append to /tmp/restore/{path}
5. set perms / xattrs from tree metadata
6. verify file BLAKE3 matches recorded
```

## Dedup logic

```
new_bytes incoming
  CDC -> chunk(hash, len)
  if store.has(hash):                     # remote/local listing
     skip upload                          # dedup
  else:
     encrypt + upload
total_uploaded = sum(new chunks * (len + envelope_overhead))
```

## Schedule + retention

```
TOML cron-like schedule:
  [snapshot]
  sources = ["/home/$USER", "/etc"]
  target = "s3://my-bucket/willo-back"
  schedule = "0 * * * *"    # hourly
  retention = { hourly = 24, daily = 7, weekly = 4, monthly = 12 }

retention pruning loop (daily):
  list snapshots desc by timestamp
  keep youngest hourly[:24]
  keep first daily[:7]
  keep first weekly[:4]
  keep first monthly[:12]
  delete other snapshots (just the index file; chunk GC follows)
chunk GC:
  reachable = ∅
  for snap in retained: reachable ∪= chunks(snap)
  for hash in store.list():
      if hash not in reachable: store.delete(hash)
```

## Repo check

```
willo-back check --deep
  for snap in snapshots:
     fetch tree
     for (path, hashes) in tree:
        for h in hashes:
            ciphertext = store.get(h)
            plaintext = age::decrypt(...)
            verify blake3(plaintext) == h
            verify length matches
  emit report; exit code 0=ok, 2=corrupt, 3=missing
```

## Failure paths

- **Network drop mid-upload** → chunk not committed; next pass retries; idempotent because content-addressed.
- **Backend `put` error** → snapshot aborted; partial state is detectable (no snapshot index written) and discarded.
- **Decrypt failure** (wrong key, corrupted chunk) → restore aborts with line item in report.
- **Out of disk** during local restore → restore halts cleanly; partial files retained for diagnosis.
- **Clock skew** → snapshot timestamps may be out of order; UI sorts by snapshot id (monotonic) not ts.

## Data structures

```rust
pub struct SnapshotIndex {
    pub id: SnapshotId,                  // hash of metadata
    pub ts: u64,
    pub parent: Option<SnapshotId>,
    pub tree_root: ChunkHash,
    pub source_paths: Vec<String>,
}

pub struct TreeNode {
    pub path: String,
    pub kind: NodeKind,                  // File | Dir | Symlink
    pub mode: u32,
    pub uid: u32, pub gid: u32,
    pub size: u64,
    pub mtime: u64,
    pub xattrs: Vec<(String, Vec<u8>)>,
    pub chunks: Vec<ChunkHash>,          // empty for Dir/Symlink
}

pub trait Store: Send + Sync {
    fn put(&self, hash: ChunkHash, bytes: &[u8]) -> Result<(), Error>;
    fn get(&self, hash: ChunkHash) -> Result<Vec<u8>, Error>;
    fn has(&self, hash: ChunkHash) -> Result<bool, Error>;
    fn delete(&self, hash: ChunkHash) -> Result<(), Error>;
    fn list_prefix(&self, prefix: u8) -> Result<Vec<ChunkHash>, Error>;
}

pub struct Repo {
    pub store: Box<dyn Store>,
    pub crypto: Crypto,                  // pin alg, recipients
    pub cdc: CdcParams,                  // pin polynomial + bounds
}
```
