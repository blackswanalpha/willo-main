# flow39 — M39: architecture & runtime flows (MAC + sandboxing + audit)

> Derived from `docs/idea.md` §12 and the work39 plan.

## Component map

```
        userspace
        +-----------------------+
        |  protected daemons    |
        |  (browser, cloud,     |
        |   mail, willo-pkg)    |
        +------+----------------+
               | profile applied at execve
               v
        +-------------------------------+
        |  kernel security:             |
        |  willomac (path MAC) | seccomp|
        |  landlock (fs)       | audit  |
        +------+--------+---------+-----+
               |        |         |
               v        v         v
        path checks  BPF filter  fs walk
                                 (ruleset fd)
                       |
                       v
        +-----------------------+
        |  /dev/audit ring buf  |
        +-----------+-----------+
                    |
                    v
        +-----------------------+
        |  willoaudit daemon    | -> §16 journal
        +-----------------------+
```

## Profile load at exec

```
execve("/usr/bin/willo-browser")
  -> elf::load
  -> security::willomac::on_execve(path, task)
       look up /etc/willomac/willo-browser.toml
       parse; install profile in task.mac_profile
       drop ambient capabilities not listed
       set kill-on-oom limits
  -> jump ring 3
```

## syscall path check (open /home/lin/.ssh/id_ed25519)

```
sys_openat(AT_FDCWD, "/home/lin/.ssh/id_ed25519", O_RDONLY)
  -> resolve dentry
  -> mac::file_check(task.profile, path, READ)
       walk profile.file_rules:
          glob_match("/home/$U/.ssh/**", path) && r? -> NO
          ... no other matching allow
       return -EACCES
  -> security::audit::log(pid, openat, path, EACCES, profile)
  -> -EACCES to user
```

## seccomp filter

```
task installs filter:
  prog = bpf:
    if syscall_nr == SYS_socket:
       if args[0] == AF_INET6: return SECCOMP_RET_ERRNO(EAFNOSUPPORT)
       return SECCOMP_RET_ALLOW
    return SECCOMP_RET_ALLOW
  install via prctl(PR_SET_SECCOMP, ...)
on syscall entry:
  bpf::run(prog, args) -> action
    ALLOW -> proceed
    ERRNO(e) -> set rax=-e and skip syscall
    KILL -> SIGSYS to task
    TRAP -> SIGSYS w/ siginfo
    TRACE -> notify ptracer
```

## Landlock restrict

```
fd = landlock_create_ruleset(NULL, 0, LANDLOCK_CREATE_RULESET_VERSION) -> v
fd = landlock_create_ruleset(&attr{handled_access_fs: READ_FILE|READ_DIR}, sizeof(attr), 0)
landlock_add_rule(fd, RULE_PATH_BENEATH, &{access:READ_FILE|READ_DIR, parent_fd: open("/usr")}, 0)
landlock_restrict_self(fd, 0)

after restrict:
  open("/etc/passwd", RDONLY) -> EACCES (not under /usr)
  open("/usr/bin/ls", RDONLY) -> ok
restrictions are inherited by children + cannot be relaxed
```

## Audit event flow

```
denial in willomac/seccomp/landlock
  -> audit::log_event(AuditEvent {
        ts, cpu, pid, uid, syscall, args_summary, decision, profile, path
     })
  -> per-CPU ringbuf push
  -> global aggregator promotes to /dev/audit FIFO
willoaudit daemon (sandboxed minimal profile):
  -> read /dev/audit
  -> format JSON event
  -> §16 journal::write(priority=NOTICE, fields={...})
  -> optional: append /var/log/audit/audit.log (rotated)
```

## "Why was this denied?" inspector

```
user clicks event in §36 settings -> Security pane
  -> reads recent audit events for pid
  -> shows: profile=willo-browser; rule: file-read /home/$U/.ssh/** = deny
  -> "Allow" button: temporary additive rule (logged separately)
  -> "Edit profile": opens /etc/willomac/willo-browser.toml in §34 editor
```

## Default profile (sketch: willo-browser)

```toml
name = "willo-browser"
exec = "/usr/bin/willo-browser"
caps = []  # no ambient
files = [
  { path = "/usr/lib/willo-browser/**", access = "rmx" },
  { path = "/etc/ssl/certs/**",         access = "r" },
  { path = "/home/$U/.cache/willo-browser/**", access = "rwm" },
  { path = "/home/$U/Downloads/**",     access = "rw" },
]
net = ["inet", "inet6"]   # tcp/udp
seccomp = "browser-renderer-default"
landlock = "browser-renderer-default"
```

## Failure paths

- **Profile missing** for protected service → kernel refuses to exec; logged.
- **Audit ringbuf overflow** → drop counter increments; alarm in §36 monitor.
- **seccomp KILL** → SIGSYS terminates task; coredump (§32) honoured if profile permits.
- **Landlock parent fd leaked** to child → still scoped (kernel snapshots ruleset on restrict).
- **Profile syntax error** → service fails to start with clear message; pkg install refuses.

## Data structures

```rust
pub struct MacProfile {
    pub name: SmolStr,
    pub caps: CapSet,
    pub file_rules: Vec<FileRule>,
    pub net_rules: NetRules,
    pub seccomp: Option<BpfProgRef>,
    pub landlock: Option<LandlockRulesetRef>,
}

pub struct FileRule {
    pub glob: GlobPattern,
    pub access: AccessBits,              // r w m x (mmap-exec)
    pub deny: bool,
}

pub enum SeccompAction {
    Allow,
    Errno(i32),
    Kill,
    Trap,
    Trace,
}

pub struct AuditEvent {
    pub ts: u64,
    pub cpu: u8,
    pub pid: Pid,
    pub uid: Uid,
    pub syscall: u16,
    pub decision: AuditDecision,         // Allow | Deny(reason)
    pub profile: SmolStr,
    pub path: Option<SmolStr>,           // truncated; ASCII safe
}

pub struct LandlockRuleset {
    pub handled_fs: AccessBits,
    pub rules: Vec<LandlockRule>,
    pub frozen: bool,
}
```
