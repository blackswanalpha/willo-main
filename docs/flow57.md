# flow57 — cross-cutting: app sandbox lifecycle

> Cross-cutting flow citing M11, M19, M24, M37, M38, M39, M41, M50.

The complete lifecycle of a sandbox profile — from author writes a daemon, to package install, to runtime enforcement, to user inspecting denials and editing policy.

## Authoring phase

```
developer writes a daemon (e.g. willofoo)
  declares default §39 profile in package:
    /etc/willomac/willofoo.toml
       caps      = []
       files     = [
         { path = "/usr/lib/willofoo/**", access = "rmx" },
         { path = "/var/lib/willofoo/**", access = "rwm" },
       ]
       net       = ["unix"]
       seccomp   = "default-daemon"   (named profile)
       landlock  = "default-daemon"
package signed (§38) and uploaded
```

## Install phase (§19 → §39)

```
willo-pkg install willofoo
  -> verify sig (§38)
  -> stage files
  -> validate /etc/willomac/willofoo.toml syntax (§39)
  -> install profile
  -> if daemon running: HUP/restart so new profile applied
post-install:
  willofoo started under profile; first execve hits §39 enforcement
```

## Runtime: execve enforcement

```
willofoo exec'd:
  kernel security::willomac::on_execve(path="/usr/bin/willofoo")
    look up profile by path
    drop ambient caps
    install seccomp filter (BPF prog)
    apply Landlock ruleset
    record (pid, profile) in task struct
  jump ring 3
on each syscall:
  seccomp::run -> ALLOW/ERRNO/KILL
on each file op:
  willomac::file_check vs profile.file_rules
on landlock-aware syscall:
  walk Landlock ruleset
on net op:
  willomac::net_check vs profile.net_rules
denials:
  -> §39 audit::log_event -> §16 journal
```

## User inspection (§36 settings → Security)

```
user opens settings -> Security -> Profiles
  list of installed profiles with status:
     willofoo: enforced; recent denials = 3
  click "willofoo": open inspector
     show profile contents (read-only)
     show recent denials with translated path/syscall
     "View profile in editor" -> §50 willedit at /etc/willomac/willofoo.toml
     "Why was this denied?" -> show rule that matched / lack of rule
```

## Profile editing (admin, with audit)

```
admin uses willedit-as-root (sudo + §39 willosudo profile to write)
  edits /etc/willomac/willofoo.toml
  saves; willomac::reload_profile validates syntax
  on success: re-evaluates running daemon's policy
  emit audit event "policy_change" with diff and admin uid
  no re-exec needed (live policy swap)
on syntax error:
  reload rejected; old policy retained
  user notified
```

## Container interaction (§41)

```
container start:
  willo-crun applies:
    - per-container seccomp from OCI config
    - Landlock ruleset from --landlock options
    - Default §39 profile "container-default"
each process inside container:
  effective profile = container-default ⊓ container-config ⊓ inherited from parent
  cannot escape via execve (Landlock + namespaces)
```

## Browser renderer (§24 + §39)

```
willo-browser main process:
  profile: willo-browser (allows net, GUI, file:downloads/cache)
fork renderer per-tab:
  parent process runs:
    seccomp_install("renderer-strict")
    landlock_restrict({ no FS, only inherited fds })
    drop net (close sockets)
    exec renderer binary
  -> §39 audit logs the tightening
  -> if renderer sandbox-escapes: kernel kills it; main spawns replacement
```

## Sudo / privilege escalation (§37)

```
willosudo command:
  PAM auth (§37)
  audit "sudo" event with command, args, uid
  setuid(0)
  drop §39 user profile; install §39 root profile (more permissive)
  exec command
on exit:
  return to caller's profile
```

## Profile development cycle

```
1. cargo new willobar
2. write daemon
3. observe denials in §16 journal as you exercise it
4. willomac aa-genprof willobar (learn-mode)
   -> traces syscalls + file ops + net ops
   -> generates a candidate profile
5. review + tighten manually
6. ship in package
```

## "Why did this fail?" workflow

```
user: "willofoo can't read /etc/willofoo.conf"
admin opens §36 Security inspector:
  filter denials by exe=willofoo
  finds: openat /etc/willofoo.conf -> EACCES (rule miss)
  click "Allow" (with audit); creates an additive rule
    /etc/willomac/willofoo.toml.d/local.toml
    { path = "/etc/willofoo.conf", access = "r" }
  policy reloaded
  daemon reads file successfully
audit log records both the denial and the override authorisation
```

## Failure paths

- **Profile syntax error on install** → install fails; user warned; no policy degradation.
- **Daemon hits permission wall in production** → audit denial; admin can override.
- **Profile + seccomp conflict** (filter denies syscall the profile expected to allow) → seccomp wins; logged as conflict.
- **Container OCI seccomp trying to allow more than host profile** → host wins; container narrowed.
- **§37 sudo without §39 profile** → §39 default-deny prevents escalation; misconfig surfaces clearly.

## Audit event shape

```json
{
  "ts": 1714530000,
  "ev": "mac.deny",
  "pid": 1432,
  "uid": 1000,
  "profile": "willofoo",
  "syscall": "openat",
  "path": "/etc/willofoo.conf",
  "decision": "deny",
  "rule_match": null
}
```

## Key invariants

1. Every protected daemon must have a `/etc/willomac/<name>.toml` profile installed by its `.willo`.
2. Profiles must be signed indirectly via §19 package sig.
3. Live profile reload never *widens* without admin authentication.
4. Containers always get a baseline `container-default` profile.
5. Sudo creates a §39 audit event with full command line.

## Tunables

- Per-user "developer mode" enables learn-mode profiling.
- Strict mode: deny on any unmatched syscall.
- Default mode: deny on protected operations only.
- Logging verbosity per profile.
