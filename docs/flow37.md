# flow37 — M37: architecture & runtime flows (Multi-user & identity)

> Derived from `docs/idea.md` §13 and the work37 plan.

## Component map

```
        +-------------+   +-------------+
        |  willologin |   |  willogreet |   (text + GUI)
        +------+------+   +------+------+
               |                 |
               v                 v
        +-------------------------------+
        |            willopam           |
        |  (auth/account/session/passwd)|
        +-+-----+-----+----+----+-------+
          |     |     |    |    |
          v     v     v    v    v
        unix  systemd keyring kerb (future)
        (sha) (logind)(§38)
                  |
                  v
        +-------------------------------+
        |        willologind            |  (session/seat tracking)
        +-------------------------------+
                  |
                  v
        +-------------------------------+
        |   user shell or compositor    |
        |   under XDG_* env vars        |
        +-------------------------------+

        +-------------+
        |  willosshd  |  (network logins -> same PAM chain)
        +-------------+
        |  willosudo  |  (privilege escalation)
        +-------------+
```

## Login flow (text)

```
boot -> tty1 -> willologin
  read username
  pam_start("login", username)
  pam_authenticate (modules: pam_unix -> shadow + argon2id verify)
  pam_acct_mgmt
  pam_open_session
     pam_systemd -> willologind RegisterSession(...)
        sets seat, tty, vt, env
  exec user shell as user uid/gid; XDG_SESSION_ID, XDG_RUNTIME_DIR set
  shell starts in $HOME
on logout:
  shell exits
  pam_close_session -> willologind.ReleaseSession
  willologin loops back to "username"
```

## GUI greeter flow

```
boot -> systemd-class target "graphical" -> willogreet on tty7
  willogreet runs willoc-comp instance ("greeter mode", no user)
  shows user list + password field
  user picks; types password
  pam_start("willogreet", user)
  pam_authenticate, pam_open_session (logind register)
  spawn user willoc-comp under DBUS_SESSION_BUS_ADDRESS
  greeter exits when user session active
```

## willosudo flow

```
willosudo apt install vim
  -> read user uid; consult /etc/willosudoers
  -> pam_start("willosudo", user)
  -> pam_authenticate (TTY prompt for password)
  -> §39 audit: "user=lin sudo cmd=apt args=install vim"
  -> setuid(0), preserve env subset (PATH, TERM, HOME)
  -> exec apt install vim
  -> on exit: drop privileges
```

## willosshd flow

```
willosshd listening on :22
  on connect:
    fork child (sandboxed §39)
    SSH handshake (ed25519 host key)
    KEX: x25519 + chacha20poly1305
    auth methods offered: publickey, password
       publickey: verify against ~/.ssh/authorized_keys (read as user uid via setuid?)
       password: pam_authenticate("willosshd", user)
    if accepted:
       exec user shell with uid/gid; SSH_CONNECTION env
    if SFTP subsystem requested:
       exec willosftp-server
```

## Session lifecycle (willologind)

```
RegisterSession(seat0, tty1, user=lin, vt=1, class=user)
  -> create /run/willologind/sessions/c1.session
       (id, uid, leader_pid, scope_cgroup_path)
  -> emit signal SessionNew
  -> active session per seat tracked
ReleaseSession(c1)
  -> kill scope cgroup (if delegated)
  -> remove session file
  -> emit SessionRemoved
TerminalSession (lock):
  -> set state Locked; compositor shows lock screen
  -> Unlock via pam_authenticate
```

## passwd change

```
willopasswd
  pam_start("passwd", user)
  pam_chauthtok:
    pam_unix prompts old + new
    enforces complexity (min len, dictionary)
    argon2id hash new pw -> /etc/shadow update (atomic rename)
    pam_keyring may update on-keyring secrets
```

## Failure paths

- **Wrong password** → pam_authenticate returns AUTH_ERR; willologin reprompts; rate-limit after 3 fails.
- **Locked account** (`!` in shadow) → pam_acct_mgmt fails; "account locked" message.
- **SSH key not in authorized_keys** → falls through to next method; final NO_MORE_AUTHS denies.
- **logind unreachable** → pam_systemd warns but does not block login (early boot).
- **Race on shadow** → file lock + atomic rename; concurrent change retries.

## Data structures

```rust
pub struct PasswdRow {
    pub username: SmolStr,
    pub uid: Uid,
    pub gid: Gid,
    pub gecos: SmolStr,
    pub home: PathBuf,
    pub shell: PathBuf,
}

pub struct ShadowRow {
    pub username: SmolStr,
    pub hash: String,                    // argon2id encoded
    pub last_change_days: u32,
    pub min_age: u32,
    pub max_age: u32,
    pub warn_days: u32,
    pub inactive_days: u32,
    pub expire_days: Option<u32>,
}

pub struct PamModule {
    pub name: &'static str,
    pub auth: Option<fn(&mut PamHandle) -> PamResult>,
    pub account: Option<fn(&mut PamHandle) -> PamResult>,
    pub session_open: Option<fn(&mut PamHandle) -> PamResult>,
    pub session_close: Option<fn(&mut PamHandle) -> PamResult>,
    pub chauthtok: Option<fn(&mut PamHandle) -> PamResult>,
}

pub struct Session {
    pub id: SmolStr,                     // c1, c2, ...
    pub uid: Uid,
    pub seat: SmolStr,                   // seat0
    pub vt: u8,
    pub class: SessionClass,             // User | Greeter | Lock | LockScreen
    pub leader_pid: Pid,
    pub state: SessionState,             // Active | Online | Closing
}
```
