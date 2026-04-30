# flow19 — M19: architecture & runtime flows

> Derived from `docs/idea.md` §15 and the work19 plan.

## Component map

```
   +------------------+   +-----------+   +---------------+
   | willoc-center    |   | willo-pkg |   | willo-updd    |
   | (GUI)            |   | (CLI)     |   | (daemon)      |
   +--------+---------+   +-----+-----+   +-------+-------+
            |                    |                 |
            +--------+-----------+                 |
                     |                             |
                     v                             v
              +-------------+              +-------------------+
              | resolver    |              | bootloader hooks  |
              +------+------+              +---------+---------+
                     |                                |
                     v                                v
              +-------------+              +-------------------+
              | repo client |              | EFI vars / sector |
              +------+------+              +-------------------+
                     |
                     v
                HTTPS / TLS
                     |
                     v
              [ signed repo ]


   on-disk:
     /var/lib/willo-pkg/db
     /etc/willo-pkg/keys/
     /.staging/<txn>/
     /var/cache/willo-pkg/
     WilloFS subvolumes:
       @rootA  @rootB  (A/B slots)
```

## Install transaction

```
willo-pkg install foo
  ├─ resolve(foo) → [foo, libbar, libqux]
  ├─ fetch each .willo over HTTPS
  ├─ verify signatures vs trust store
  ├─ snapshot @rootA → @snap-pre-foo
  ├─ stage tree under /.staging/txn-N/
  ├─ run pre-install hook in sandbox
  ├─ atomic rename: /.staging/txn-N/* → @rootA/*  (CoW: cheap)
  ├─ run post-install hook
  ├─ db.add(package: foo, files: [...], sha: ...)
  └─ commit; on any error: rollback to @snap-pre-foo
```

## Upgrade with A/B slots

```
willo-updd: scheduled check
  ├─ ask repo for kernel + base set
  ├─ if newer: download to @rootB
  ├─ verify signatures
  ├─ snapshot @rootB → @snap-pre-upgrade
  ├─ install into @rootB
  ├─ set firmware: boot_next = B; boot_count = 0; boot_max = 3
  └─ reboot
```

## Boot path with rollback

```
bootloader on power-on:
  read slot, count, max
  if count >= max:
     log "rollback"; swap active slot; count = 0
  count += 1; persist
  load kernel from active slot
  ...
kernel after fully started (e.g. 60 s OK, services healthy):
  firmware.set(count = 0)              # commit success
```

## Signature verification

```
package.willo
  ├─ manifest.toml + files.tar
  └─ sig.ed25519
verify:
  hash = sha256(manifest.toml || tree_hash(files.tar))
  ed25519::verify(pubkey, hash, sig)
trust:
  pubkey ∈ /etc/willo-pkg/keys/ (system) or
          ∈ user-keyring (per-user, opt-in)
```

## Resolver

```
input:  request (name, range)
        installed packages
        repo index
solver: SAT/PB with cost = (newer-version > older-version > install < remove)
output: actions = install [...], remove [...], update [...]
```

Conflict messages cite the chain (`foo 1.2 wants libbar < 2; baz 0.4 wants libbar >= 2`).

## DB layout

```
/var/lib/willo-pkg/
├── db.json           # { name -> { version, files, deps, sha } }
├── txn.log           # append-only history
└── keys -> /etc/willo-pkg/keys/
```

## Failure paths

- Signature mismatch → install aborts before staging touches root.
- Hook timeout → kill, abort, rollback.
- Disk full mid-install → abort + rollback (snapshot makes this cheap).
- Bad upgrade boots → rollback after `boot_max` attempts.
- Resolver unsat → user-friendly conflict report; no changes made.
