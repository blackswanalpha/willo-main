# flow26 — M26: architecture & runtime flows (Linux compatibility)

> Derived from `docs/idea.md` §18 and the work26 plan.

## Component map

```
        userspace
        +---------------------+        +-------------------+
        | upstream Linux ELF  |  exec  | willo-linuxrun    |
        | (bash, coreutils)   |<------>| chroot helper     |
        +---------+-----------+        +-------------------+
                  |                            |
                  v                            v
        +-------------------------------------------------+
        |   kernel: process.aux = LinuxAux                |
        |   alternate LSTAR -> compat::linux::dispatch    |
        +---------+-------------------+-------------------+
                  |                   |
                  v                   v
        +-------------------+   +------------------+
        |  syscall table    |   |  fake vDSO page  |
        |  (mapped/emul)    |   |  + AT_SYSINFO    |
        +---------+---------+   +------------------+
                  |
                  v
        +-------------------+
        |  /proc, /sys      |  (linux overlay; files/dirs synthesised)
        +-------------------+
```

## ELF load (linux-aux)

```
execve(/opt/linux-rootfs/bin/bash)
  -> elf::load
       PT_INTERP = /lib64/ld-linux-x86-64.so.2
       -> mark process.aux = LinuxAux
       -> map glibc ld.so from rootfs
       -> aux vector includes:
            AT_SYSINFO_EHDR = vdso_addr
            AT_HWCAP, AT_PLATFORM = "x86_64"
            AT_RANDOM = 16 bytes
       -> jump to ld.so entry, ring 3
ld.so resolves libc.so.6 (in rootfs)
bash main starts
```

## Syscall dispatch (linux-aux)

```
guest userspace: syscall (rax = SYS_write_linux = 1)
  -> kernel: LSTAR (aux selector) -> compat::linux::dispatch
       table[1] = sys_write_linux_passthrough
       -> calls native willoc::write(fd, buf, len)
       -> rax = bytes_written
  -> sysretq
```

For long-tail call (e.g. `landlock_create_ruleset`):

```
syscall (rax = 444)
  -> compat::linux::dispatch
       table[444] = emulate_landlock
       -> bridges to §39 willomac::create_ruleset
       -> rax = ruleset_fd
  -> sysretq
```

For unmapped:

```
syscall (rax = 999)
  -> table[999] = handle_enosys
       -> log "linux compat ENOSYS for syscall 999, pid=X"
       -> rax = -ENOSYS
  -> sysretq
```

## vDSO flow

```
glibc gettimeofday()
  -> looks up __vdso_gettimeofday from vdso symbol table
  -> direct call (no syscall)
  -> reads kernel-shared monotonic counter from vdso page
  -> returns
   (no ring transition, ~10x faster than syscall path)
```

## /proc translation example

```
bash: cat /proc/self/maps
  -> open("/proc/self/maps")
  -> VFS resolve: prefix /proc → ProcFs (Linux overlay if pid.aux=LinuxAux)
  -> ProcLinux::self_maps(pid)
       walk pid.address_space.vmas
       format each:
         "{start:016x}-{end:016x} {prot} {off:08x} 00:00 0  {path}"
  -> bytes returned
```

## Signal translation

```
Linux SIGCHLD = 17 (Linux numbering)
Willo Sig::Child = enum variant

deliver:
  willo signal raised
  -> if target.aux == LinuxAux:
       linux_signum = sig_to_linux(Sig::Child) // 17
       insert into linux signal frame on user stack
       set rdi = 17
  -> resume to handler
```

## Failure paths

- **Unknown syscall** → ENOSYS + journal warning; binary often falls back gracefully (e.g. uses `read`/`write` if `io_uring_*` fails).
- **glibc TLS init fails** → ld.so prints "FATAL: kernel too old"; mitigation: bump fake `uname` release.
- **`/proc/self/exe` mismatch** → some apps `realpath` it then verify; emulator returns the rootfs-relative path consistently.
- **Bad clock_gettime** → non-monotonic time from vDSO; surface kernel-side test that flags any non-monotonic delta.
- **Robust list crash on exit** → kernel `set_robust_list` honors the list; race-tested in `clone3`-stressing tests.

## Data structures

```rust
pub enum ProcessAux {
    Native,
    LinuxAux,
}

pub struct LinuxSyscallEntry {
    pub name: &'static str,
    pub handler: fn(SyscallArgs) -> Result<u64, Errno>,
}

pub struct VdsoPage {
    pub clock_mono_offset: AtomicU64,    // updated each tick
    pub gettimeofday_offset: AtomicU64,
    pub _padding: [u8; 4096 - 16],
}

pub struct LinuxAuxVector {
    pub at_sysinfo_ehdr: VirtAddr,
    pub at_hwcap: u64,
    pub at_platform: &'static str,
    pub at_random: [u8; 16],
}
```
