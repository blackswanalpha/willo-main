# flow52 — cross-cutting: user login → desktop ready

> Cross-cutting flow citing M11, M17, M22, M28, M30, M31, M35, M37, M38, M40, M44.

Picks up where flow51 hands off (login screen visible). Documents the chain from "user types password" to "desktop fully ready, all sync daemons online, a11y plumbed".

## Sequence

```
T+0    greeter has focus; user types password
T+~10  pam_authenticate (M37)
        pam_unix: argon2id verify against shadow
        pam_keyring: prep keyring for next session_open
T+~30  pam_acct_mgmt (account status checks)
T+~50  pam_open_session
        pam_systemd: willologind RegisterSession
            seat0, vt7, class=user, scope cgroup created
        pam_keyring: unlocks user keyring (FDE-style, per-user)
        pam_secrets: unlocks default SecretService collection (§38)
T+~80  spawn user willoc-comp under user UID
        compositor inherits wl_display socket dir from /run/user/$uid
T+~100 user compositor binds:
        zwlr_layer_shell, xdg_shell, wl_seat, wl_output
        a11y bus: claim org.a11y.atspi.Registry (§31)
T+~120 willofs::mount(/home/$user)
        crypt block layer activates per-user volume key (§38)
T+~150 systemd-class user target "graphical.target" begins
T+~180 autostart units (parallel):
         pipewire / willopipe (§40)
         willortkit (§44 if pro-audio enabled)
         willoc-cloud (§28)
         willomail-sync (§35)
         willocal sync (§35)
         willocontacts sync (§35)
         willoc-notify (§36)
         willo-a11y (§31, if user opted in)
         willo-osk (§31, if no physical keyboard)
T+~250 desktop shell: panel, app launcher, status indicators visible
T+~300 §28 cloud daemon online; ~/Cloud/* virtual mounts ready
        §35 sync first poll (staggered backoffs)
T+~500 ms typical "feels-ready" mark
T+~30s mark-good handoff to §47 willobootctl
```

## Authentication pipeline (M37 + M38)

```
PAM stack /etc/pam.d/willogreet:
  auth     required pam_unix.so      -> argon2id verify
  account  required pam_unix.so
  session  required pam_systemd.so   -> register session, set XDG vars
  session  optional pam_keyring.so   -> kernel keyring + secrets
  session  optional pam_keyinit.so   -> per-session keyring slot
  password required pam_unix.so
```

## Per-user FDE unlock

```
user password derives a wrapping key (Argon2id; same hash as PAM but separate key slot):
  wrap = argon2id(password, user.salt, params)
LUKS slot for /home/$user opened with `wrap`
crypt block layer activated; mount /home/$user
prior to logout: crypt::lock() overwrites key in keyring
```

## a11y session bring-up (M31)

```
willoc-comp claims org.a11y.atspi.Registry
if user.preference.screen_reader == on:
  systemd-class autostart: willo-a11y
    subscribe to focus events
    spawn willo-tts (espeak)
if user.preference.osk_when_no_kbd == on:
  inspect input devices
  if no physical keyboard: launch willo-osk
high-contrast theme applied if set
```

## Audio session (M18 + M40 + optional M44)

```
willopipe daemon starts under user UID
if pro-audio mode (M44):
  willortkit grants RT priority to willopipe + willojack threads
  threadirq path ensures audio IRQs preempt other work
default mixer node "speaker" + "headphones" exposed
PulseAudio-class compat layer (legacy apps work)
```

## Sync daemons (M28 + M35)

```
willoc-cloud loads accounts from §28 OnlineAccounts (D-Bus)
  per account:
    if access_token expired -> refresh
    start poll loop with staggered backoff
willomail-sync similar; on push-capable accounts (JMAP) opens WebSocket
network-aware: if interface up but no DHCP yet, wait for "net.ready" signal
```

## Notification toast bring-up (M36)

```
willoc-notify claims org.freedesktop.Notifications
honor user.preference.quiet_hours
welcome toast on first login: "Welcome back, $user"
```

## Desktop shell (M17 + M36)

```
willoc-shell (top panel + app launcher + tray) launches
status icons connect to:
  net (M14)
  battery (M23)
  audio (M40)
  notifications (M36)
  cloud (M28)
  bluetooth (M43, if enabled)
panel becomes interactive ~T+250
```

## Failure paths

- **Wrong password** → PAM AUTH_ERR; greeter reprompts; lockout after 3 fails.
- **logind unreachable** → pam_systemd warns; session registered later; non-fatal.
- **§38 keyring fails to unlock** → user prompted to repair; system logs in but secrets-dependent apps degraded.
- **Compositor crash** mid-session → systemd-class user manager respawns; user re-presents lock screen.
- **Network offline** → §28 + §35 backoff; UI shows "syncing paused".
- **Pro-audio mode requested without RT kernel** → falls back to default; warn in audio settings.

## Lifecycle to logout

```
user picks "Log out":
  user systemd target "graphical.target" stops
  willoc-comp closes; clients gracefully exit (per session manager protocol)
  cloud + mail daemons flush state
  pam_close_session:
    pam_secrets: re-lock collections
    pam_keyring: drop per-session keys
    pam_systemd: ReleaseSession
  /home/$user crypt locks (key wiped)
  greeter reappears
```

## What "ready" means

- Compositor visible.
- Network reachable (HTTP test).
- Audio mixer available.
- Notifications working.
- Sync daemons running (may not have completed first poll).
- a11y available if requested.

## Tunables

- Auto-login user via §37 config (skip greeter, with audit).
- Stagger autostart with `start-after.toml` per unit.
- Quiet hours times.
- Pro-audio toggle.
