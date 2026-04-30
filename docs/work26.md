# work26 — M26: Linux compatibility (pico-process-style ELF + syscall translator, fake vDSO, /proc shim)

> Derived from `docs/idea.md` §18 (Virtualization & Containers — WSL-class).

## Goal

Run unmodified Linux ELF binaries on Willo without a full guest kernel — the WSL1 trick. Add a per-process **"linux-aux"** mode, an alternate **syscall entry path** routing to `compat::linux::dispatch`, a tabular Linux→Willo syscall map plus emulation for the long tail, a fake **vDSO** providing `gettimeofday`/`clock_gettime`/`getcpu`, and a `/proc`/`/sys` translation overlay so common tools (`bash`, `coreutils`, `apt-get`) think they are home.

## Depends on

- **M11** — process model + syscall ABI.
- **M16** — procfs/sysfs (extend with linux overlay).
- **M12** — signals + IPC (Linux signal numbers map to Willo).
- **M28** (optional) — chrooted Ubuntu rootfs convenience for testing.

## Acceptance criteria

- [ ] `linux-x86_64` ELF binaries load via `elf::load` with `LinuxAux` mode and execute.
- [ ] Top 100 Linux syscalls covered (read/write/openat/close/mmap/clone/wait4/execve/…); rest return `ENOSYS` with a logged event.
- [ ] Fake vDSO page mapped at the address advertised in AT_SYSINFO_EHDR; `clock_gettime` works.
- [ ] `/proc/self/maps`, `/proc/self/exe`, `/proc/cpuinfo`, `/proc/meminfo` shapes match Linux closely enough for `htop` and `ps aux` to render.
- [ ] Upstream Ubuntu `bash` + GNU `coreutils` (`ls`, `cat`, `cp`, `mv`, `rm`, `grep`) run without modification.
- [ ] `apt-get update` reaches a Linux APT repo and downloads `Packages.gz` (no install yet).
- [ ] `kernel/tests/linux_compat_bash.rs` runs upstream `bash` and pipes a `for` loop with redirection.
- [ ] Compat matrix doc `docs/compat-matrix.md` enumerates which Ubuntu binaries are tested.

## Task breakdown

### T1. Process flag — `kernel/src/process.rs`
- Add `aux: ProcessAux` enum: `Native | LinuxAux`.
- Set on `execve` if ELF interp matches `/lib64/ld-linux-x86-64.so.2`.

### T2. Linux syscall entry — `kernel/src/compat/linux/entry.rs` (new)
- Alternate `LSTAR` target if `aux == LinuxAux`; route to `compat::linux::dispatch(rax, rdi, rsi, rdx, r10, r8, r9)`.

### T3. Syscall mapping table — `kernel/src/compat/linux/table.rs`
- `static SYSCALLS: [Handler; 512]`.
- Each entry: native passthrough (most), emulation (long tail), or `ENOSYS`.

### T4. Long-tail emulators — `kernel/src/compat/linux/emul/`
- `clone3`, `pidfd_*`, `io_uring_*` (best-effort), `landlock_*` (forwards to §39).
- Linux signal numbers → Willo signals translation table.

### T5. Fake vDSO — `kernel/src/compat/linux/vdso.rs`
- Pre-baked ELF page with `__vdso_clock_gettime`, `__vdso_gettimeofday`, `__vdso_getcpu`, `__vdso_time`.
- Mapped read-only into linux-aux processes; `AT_SYSINFO_EHDR` aux vector points at it.

### T6. `/proc` translation overlay — `kernel/src/fs/proc/linux/`
- Synthesise `cpuinfo`, `meminfo`, `stat`, `loadavg`, `mounts`, `self/maps`, `self/exe`, `[pid]/stat`, `[pid]/status`.
- Reuse Willo data sources (M16 scheduler stats, §13 cpuinfo).

### T7. `/sys` translation overlay — `kernel/src/fs/sys/linux/`
- Minimum: `/sys/devices/system/cpu/`, `/sys/class/net/`, `/sys/class/power_supply/` shapes.

### T8. ELF loader differences — `kernel/src/elf.rs`
- Recognise `PT_INTERP == /lib64/ld-linux-x86-64.so.2`; map a Willo-side stub linker that hands off to glibc dynamic linker once present.
- Support glibc's `robust list` and `set_robust_list` syscall.

### T9. Userspace runtime — `userspace/linux-rootfs/`
- Stage a chrooted Ubuntu rootfs under `/opt/linux-rootfs/`.
- `willo-linuxrun <bin>` chroots and executes inside the rootfs.

## New / modified files

| Path | Change |
| --- | --- |
| `kernel/src/process.rs` | + `aux` field |
| `kernel/src/compat/linux/*` | **new** |
| `kernel/src/elf.rs` | + linux-aux mode + robust list |
| `kernel/src/fs/proc/linux/*` | **new** |
| `kernel/src/fs/sys/linux/*` | **new** |
| `userspace/linux-rootfs/` | **new** stage helper |
| `docs/compat-matrix.md` | **new** running list |

## Tests to add

- `kernel/tests/linux_compat_bash.rs` — bash `for` loop + redirection.
- `kernel/tests/linux_compat_coreutils.rs` — `ls -la /` matches a Linux baseline (modulo paths).
- `kernel/tests/linux_compat_clock.rs` — vDSO `clock_gettime` returns monotonic time consistent with Willo's clock.
- `kernel/tests/proc_self_maps.rs` — Linux-aux process reading `/proc/self/maps` matches actual VMA layout.

## Risks & open questions

- **Long tail of Linux syscalls** — track in `compat-matrix.md`; mark gaps "expected" until a user hits them.
- **glibc dynamic linker** — easier than expected if we host Ubuntu's own `ld-linux-x86-64.so.2`; ship it in the chroot.
- **`/proc` shape drift** — Linux changes proc files between releases; pin one Ubuntu LTS as canonical.
- **io_uring** — deferred; emulate as `ENOSYS` so apps fall back to readv/writev.
- **Performance** — every Linux syscall takes the slow path; acceptable for compatibility, optimize hottest 10 later.
- **Security** — Linux binaries do not run under a §39 profile by default; opt-in until the policy story is mature.
