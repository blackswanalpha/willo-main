# work37 — M37: Multi-user & identity (passwd/shadow argon2id, login manager, session, sudo, SSH server)

> Derived from `docs/idea.md` §13 (Multi-user & Identity).

## Goal

Make Willo a true multi-user OS. Land `passwd`/`shadow` with **argon2id** hashes, a modular **PAM-class** auth library, a **getty-class** text login + GUI greeter, a **session manager** (logind-class, tracking seats/sessions), a **`sudo`/`doas`** tool, a userland **OpenSSH-class server** (`willosshd`) with SFTP, and home-dir provisioning with `/etc/skel`.

## Depends on

- **M11/M12** — userspace + processes/threads.
- **M14** — netstack (SSH).
- **M17** — compositor (greeter).
- **M38** — keyring (`willopam` consumes it).

## Acceptance criteria

- [ ] `/etc/passwd`, `/etc/group`, `/etc/shadow` present and correct after install.
- [ ] `willopasswd` creates user with argon2id hash; `willologin` authenticates against shadow.
- [ ] PAM-class stack supports `auth`, `account`, `session`, `password` module categories.
- [ ] Text greeter (getty-class) appears on tty1; GUI greeter appears on tty7 when compositor available.
- [ ] `willologind` tracks sessions; `loginctl list-sessions`-class command works.
- [ ] `willosudo cmd` prompts; verifies via PAM; logs to §39 audit.
- [ ] `willosshd` accepts password + pubkey; SFTP subsystem works; sandboxed via §39 profile.
- [ ] `kernel/tests/multiuser_login.rs` (userspace integration) creates user, logs in, runs `id`, logs out.

## Task breakdown

### T1. user/group databases — `userspace/willoc-id/`
- File parsers + writers for `/etc/{passwd,group,shadow,gshadow}`.
- `willouseradd`/`willouserdel`/`willopasswd` CLIs.

### T2. argon2id hashing — `userspace/willopam/argon2.rs`
- Pin params (m=65536, t=3, p=4) per OWASP 2026.
- Random salt 16 bytes.

### T3. PAM-class library — `userspace/willopam/`
- Module ABI: `pam_authenticate`, `pam_acct_mgmt`, `pam_open_session`, `pam_close_session`, `pam_chauthtok`.
- Stack config in `/etc/pam.d/<service>`.
- Default modules: `pam_unix` (shadow), `pam_systemd` (session), `pam_keyring` (§38).

### T4. nsswitch — `userspace/willoc-nss/`
- `/etc/nsswitch.conf` directs passwd/group lookups.
- v1: `files` only; LDAP/SSSD deferred.

### T5. logind-class session manager — `userspace/willologind/`
- Tracks seats (display + input), sessions (one per login).
- D-Bus API; `loginctl`-class CLI.
- Hooks `pam_systemd` to register sessions.

### T6. text + GUI greeters — `userspace/willologin/`, `userspace/willogreet/`
- text greeter on tty1; getty-class.
- GUI greeter on tty7; compositor client; password + biometric stub.

### T7. sudo/doas — `userspace/willosudo/`
- PAM auth chain; preserves env carefully; audit log.
- Configurable via `/etc/willosudoers` (visudo-class editor).

### T8. SSH server — `userspace/willosshd/`
- ssh-rs-class implementation (OpenSSH protocol); password + pubkey + ed25519 host keys.
- SFTP subsystem.
- Runs under §39 profile; per-connection child processes.

### T9. home-dir provisioning — `userspace/willouseradd/skel.rs`
- Copy `/etc/skel/*` to new user's home; set ownership.

## New / modified files

| Path | Change |
| --- | --- |
| `userspace/willoc-id/` | **new** |
| `userspace/willopam/` | **new** |
| `userspace/willoc-nss/` | **new** |
| `userspace/willologind/` | **new** |
| `userspace/willologin/` | **new** |
| `userspace/willogreet/` | **new** |
| `userspace/willosudo/` | **new** |
| `userspace/willosshd/` | **new** |
| `kernel/src/process.rs` | + uid/gid/groups context |
| `kernel/src/syscall.rs` | + setuid/setgid/setgroups/getuid/getgid |

## Tests to add

- `kernel/tests/setuid_basic.rs` — uid switch + groups.
- `userspace/willopam/tests/argon2id_vectors.rs` — RFC 9106 vectors.
- `userspace/willologind/tests/session_track.rs` — login → session created → logout → cleaned.
- `userspace/willosshd/tests/pubkey_auth.rs` — ed25519 key login.
- `kernel/tests/multiuser_login.rs` — full create/login/run/logout.

## Risks & open questions

- **pam_systemd ↔ logind** circular dep — break by allowing logind to start without PAM during early boot, then re-handshake.
- **Shadow file race** — file lock + atomic rename; coordinate with §16 fsync semantics.
- **SSH host keys** — generate at first boot; persist in `/etc/willossh/`.
- **Biometric** — stub trait now (`AuthBackend`); real fingerprint reader deferred to §40 sensor work.
- **Argon2 params** — bump per OWASP guidance every 2 years.
- **NSS plug** — LDAP/SSSD a future M37.x; document.
