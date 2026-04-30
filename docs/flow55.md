# flow55 — cross-cutting: privacy / data lifecycle map

> Cross-cutting reference document — where every byte of user data lives and crosses milestones. Touches §12, §16, §19, §20, §27, §28, §35, §38, §39, §49.

This is the privacy-and-data-control map a contributor or reviewer reaches for to answer "if a user types or stores X, where does it actually go?". It's deliberately exhaustive about handoffs.

## At-rest storage classes

```
class                            location                                  encryption
---------------------------------------------------------------------------------------
system files (read-only)         WilloFS root (slot a/b)                   FDE LUKS2 (§38)
user home                        /home/$user/                              per-user crypt
secrets (OAuth, app passwords)   /var/lib/willoc-secrets/                  age + user key
SSH keys                         /home/$user/.ssh/                         file perms + FDE
cloud cache                      /home/$user/.cache/willoc-cloud/          FDE
mail/calendar/contacts SQLite    /home/$user/.local/share/willomail/       FDE
backup chunks                    target (local/WebDAV/S3)                  age (§27)
crash dumps                      /var/crash/                               FDE; perms 0700
audit log                        §16 journal                               FDE
package metadata                 /var/lib/willo-pkg/                       FDE
TPM-sealed master key            in TPM NVRAM, plus LUKS slot              hardware-bound
```

## In-flight data (network)

```
type                  protocol                  cert chain
-------------------------------------------------------------------
package fetch         HTTPS + ed25519           §38 trust root
update slot bytes     HTTPS + ed25519           §38 trust root
mail (JMAP/IMAP)      TLS                       system CA + per-acct
cal/contacts (DAV)    TLS                       system CA
cloud sync            TLS                       system CA + OAuth
WireGuard tunnels     ChaCha20Poly1305          per-peer keys (§42)
WiFi data plane       CCMP/GCMP                 4WH PSK or SAE (§21)
SSH                   chacha20+x25519           host key + user key (§37)
recovery image dl     HTTPS + ed25519           §38 trust root
crash report upload   HTTPS + ed25519 sig       §38 user key (opt-in)
```

## OAuth / token handling (§28)

```
acquisition:
  PKCE flow in browser (§24)
  redirect URL = http://127.0.0.1:<port>/callback (loopback only)
  authorization code -> access_token + refresh_token
storage:
  willoc-accounts D-Bus daemon
  tokens persisted via §38 SecretService
  access_token in keyring (kernel) for hot use; never on disk in plaintext
refresh:
  on near-expiry, foreground refresh; backgrounds wake §23 hooks too
exposure:
  apps cannot read raw tokens — they call willoc-accounts which proxies
  per-app scope tracking; revoke per-app
revocation:
  user revokes -> token deleted from keyring; backend revoke endpoint hit
```

## Telemetry posture (§19, §29, §32)

```
default: NO telemetry leaves the box
opt-in classes (each separately togglable):
  - pkg update channel telemetry (which versions installed, anonymized)
  - crash report (per-crash explicit OK)
  - perf flamegraphs to vendor (extremely rare; advanced toggle only)
implementation:
  every potential exit point declares its purpose in /etc/willo/telemetry.toml
  §39 audit records every send + sender + dest
```

## User data deletion semantics

```
willo-delete-user lin:
  pam_close_session if logged in
  shred user crypt key (overwrite keyring, destroy LUKS slot)
  rm -rf /home/lin (now meaningless, ciphertext)
  free per-user containers (§41)
  detach §28 accounts, revoke tokens
  §16 journal: emit user-deleted event (no PII)
backup:
  user data in §27 chunks remains until repo prune; warn user.
file history:
  removed alongside backup chunks at next prune.
```

## Logs and what they contain

```
§16 journal
  events: services, audit denials, login/logout, PAM, network state, FDE unlock
  redacts: passwords, tokens, secret bytes
  retention: 30 days (configurable); rotated to disk
§39 audit log
  every MAC/seccomp/Landlock denial
  every privilege escalation
  every signed package transaction
crash dumps
  may contain stack memory; treat as secret
willoc-cloud + mail logs
  metadata only; no body content unless --debug
```

## "Permission portals" surface

```
screen capture (§40 willopipe) requires per-session portal grant
camera (§40 V4L2)              same
location (future)              same
notifications (§36)            per-app allow
clipboard cross-app peeks      transient grant
file picker outside sandbox    portal-mediated path widening
```

## §39 default profiles per data class

```
profile                     can read                 can write
willo-browser               cache, downloads          cache, downloads (no SSH key)
willomail                   mail SQLite               mail SQLite
willoc-cloud                ~/Cloud/* + secrets-fetch ~/Cloud/*
willotrace                  /sys, /proc               none persistent
willo-back                  source paths (read)       backup repo target only
```

## Cross-cutting privacy invariants

1. No secret bytes ever appear in §16 journal, §39 audit log, crash dumps, or telemetry payloads.
2. OAuth tokens never on disk in plaintext (§28 + §38).
3. SSH private keys never leave the device (§37 always uses agent or in-memory).
4. Backup repo is mandatorily encrypted; turning it off is gated by a UI confirm.
5. Containers (§41) inherit `/home/$user` only via explicit bind mount; default is empty.
6. Recovery shell (§49) requires owner authentication before mounting `/home`.
7. Migration tools (§49) write only to target; never modify source partitions.
8. Telemetry off by default; never re-enabled by an update.

## Failure paths

- **Token leak via app log** → §39 audit catches reads of `willoc-secrets` socket; surfaced.
- **Plaintext token cached by app** → covered by static analysis (rust-clippy custom lints in CI); doc'd as forbidden pattern.
- **Backup target compromised** → chunk encryption protects content; metadata size patterns visible (acknowledged limitation).
- **Crash dump uploaded with secret in stack** → user previews + redacts before upload.
- **Telemetry endpoint compromised** → only ed25519-signed payload accepted; vendor signs replies.

## What is NOT yet covered (future work)

- Differential privacy noise on aggregate metrics (telemetry).
- Hardware-bound app-level secrets (TPM-sealed per-app keys).
- Anonymous credentials (BBS-class) for ratings/reviews.
- Cross-device encrypted clipboard.
- Encrypted swap (§5/§38 — design pending).
