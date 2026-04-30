# work39 — M39: MAC + sandboxing + audit (AppArmor-class profiles, seccomp-bpf, Landlock, auditd)

> Derived from `docs/idea.md` §12 (Security & Privilege).

## Goal

Layer defense-in-depth on top of Willo's process model. Land a **path-based MAC** profile language (AppArmor-class), a **seccomp-bpf-class** syscall filter, a **Landlock-class** per-process FS sandbox via ruleset fds, and a kernel **audit ring buffer** + userspace **`willoaudit`** daemon that streams denials to the §16 journal. Ship default profiles for the §19 update daemon, §29 willotrace, §28 cloud sync, §24 browser, and §35 mail.

## Depends on

- **M11/M12** — process model + syscalls.
- **M16** — system journal (sink for audit).
- **M29** — eBPF (powers seccomp-bpf — same JIT/verifier).
- **M37** — multi-user (per-user profiles).

## Acceptance criteria

- [ ] `willomac` profile language defined; profiles in `/etc/willomac/`.
- [ ] Profile applied at `execve` by path; deny-by-default for in-tree daemons.
- [ ] `seccomp_install(prog)` syscall installs a BPF filter on current process; deny returns SIGSYS.
- [ ] `landlock_create_ruleset` + `landlock_add_rule` + `landlock_restrict_self` syscalls work.
- [ ] Audit ring buffer; `willoaudit` daemon writes denial events with full context to journal.
- [ ] Default profiles loaded for: `willo-pkg`, `willo-updd`, `willotrace`, `willoc-cloud`, `willo-browser`, `willomail`, `willocompositor`.
- [ ] `kernel/tests/seccomp_deny.rs` — filter-denied syscall sends SIGSYS.
- [ ] `kernel/tests/landlock_fs.rs` — restricted process cannot read forbidden path.
- [ ] `kernel/tests/audit_ringbuf.rs` — denial logged with pid, syscall, path.

## Task breakdown

### T1. willomac profile language — `kernel/src/security/willomac/`
- TOML profile: name, exec path, capabilities, file rules (`r`, `w`, `m`, `x`), network rules.
- Path globbing: `**` recursive, `*` segment, `?` char.

### T2. willomac enforcement — `kernel/src/security/willomac/enforce.rs`
- Hook `execve`: load profile by exec path; clone into `task->mac_profile`.
- File ops (`openat`, `read`, `write`, `mmap`-X): check `task->mac_profile.file_rules`.
- Network ops: check by family/proto.
- Default deny if profile missing for protected service classes.

### T3. seccomp-bpf-class — `kernel/src/security/seccomp.rs`
- Reuse §29 BPF verifier + JIT.
- Filter program runs at syscall entry; returns ALLOW / KILL / TRAP / ERRNO / TRACE.
- Per-task chain (oldest filter applies first).

### T4. Landlock-class — `kernel/src/security/landlock.rs`
- Ruleset fd; `add_rule(ruleset, rule)` builds a tree of allowed (path, access) pairs.
- `restrict_self` applies; never relaxable for the task.

### T5. Audit ring buffer — `kernel/src/security/audit.rs`
- Per-CPU ringbuf + global aggregator; events stamped with pid, uid, syscall, decision, profile.
- Reader file `/dev/audit`.

### T6. willoaudit daemon — `userspace/willoaudit/`
- Reads `/dev/audit`; structures + writes to §16 journal.
- Optionally also writes to `/var/log/audit/audit.log`.

### T7. Default profiles — `userspace/willomac-profiles/`
- One file per protected service. Versioned in repo. Installed by §19 packaging.

### T8. Settings + UI — `userspace/willoc-settings/plugins/security.rs`
- Show currently active profile per process; "Why was this denied?" inspector.

## New / modified files

| Path | Change |
| --- | --- |
| `kernel/src/security/willomac/*` | **new** |
| `kernel/src/security/seccomp.rs` | **new** |
| `kernel/src/security/landlock.rs` | **new** |
| `kernel/src/security/audit.rs` | **new** |
| `kernel/src/syscall.rs` | + seccomp + landlock + audit syscalls |
| `userspace/willoaudit/` | **new** |
| `userspace/willomac-profiles/` | **new** |
| `userspace/willoc-settings/plugins/security.rs` | **new** |

## Tests to add

- `kernel/tests/willomac_deny.rs` — daemon under profile cannot read forbidden file.
- `kernel/tests/seccomp_deny.rs` — filter-denied syscall sends SIGSYS.
- `kernel/tests/landlock_fs.rs` — restricted process gets EACCES for forbidden path.
- `kernel/tests/audit_ringbuf.rs` — denial event captured.
- `userspace/willoaudit/tests/journal_format.rs` — journal entry shape stable.

## Risks & open questions

- **Profile authoring burden** — keep the language minimal; ship `willomac aa-genprof`-class learn-mode for new daemons.
- **Performance** — every syscall walks profile checks; bench on hot path; cache resolved decisions per fd.
- **Audit ringbuf overflow** — back-pressure + drop counter + alarm; surface in §36 system monitor.
- **Conflict with §38 keyring** — denied secrets access logged; never log secret bytes themselves.
- **Stacking with §26 Linux compat** — Linux binaries can opt into Landlock natively; test parity.
- **Path-based vs label-based** — path-based v1; label-based (SELinux-class) is a future M39.x.
