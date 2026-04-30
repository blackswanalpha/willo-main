# flow11 — M11: architecture & runtime flows

> Derived from `docs/idea.md` §4, §9 and the work11 plan.

## Component map

```
ring 0 (kernel)                   ring 3 (user)
+----------------------+          +----------------+
|  syscall_entry stub  |  <-----  |  init / sh     |
|  swapgs / save regs  |          | (ELF binary)   |
+----------+-----------+          +-------+--------+
           |                              ^
           v                              | sysretq
+----------------------+                  |
|  syscall_dispatch    | -----------------+
|  ┌──────────────┐    |
|  │ table[rax]   │    |
|  └──────────────┘    |
+----+-----+-----+-----+
     |     |     |
     v     v     v
   VFS  process  usercopy
 (M10)   table    (#PF fixup)
```

## Boot extension (deltas vs. M10)

1. M10 boot completes (mounts ready, framebuffer ready, ATA ready).
2. `gdt::install_user_segments()`.
3. `syscall::init()` — write `STAR`/`LSTAR`/`FMASK`/`EFER.SCE`, install entry stub.
4. `process::spawn_init("/sbin/init")`:
   - `Process::new()` with `AddrSpace::new_user()`.
   - `elf::load("/sbin/init", &mut addr_space)` → entry, base.
   - Anon-map user stack (top of user half, 1 MiB initial).
   - Build argv/envp/auxv on stack.
   - Set saved regs: `RIP=entry`, `CS=USER_CODE`, `RSP=stack_top`, `SS=USER_DATA`, `RFLAGS.IF=1`.
   - First context switch: `iretq` into ring 3.
5. Kernel-resident shell stays as a debug fallback bound to serial.

## Syscall round-trip

```
user:    mov rax, 1      ; SYS_write
         mov rdi, 1      ; fd
         lea rsi, [msg]
         mov rdx, len
         syscall

cpu:     CS/SS  <- STAR.kernel
         RIP    <- LSTAR
         RFLAGS &= ~FMASK

kernel:  swapgs                        ; per-CPU base
         save user rsp -> per-CPU
         load kernel rsp <- per-CPU
         push user regs
         call syscall_dispatch
           -> table[1] = sys_write
              -> usercopy::copy_from_user(rsi, rdx)
              -> file::write(fd=rdi, ...)
         pop regs
         restore user rsp
         swapgs
         sysretq

cpu:     RIP    <- rcx
         RFLAGS <- r11
         CS/SS  <- STAR.user
```

## ELF load flow

```
elf::load(path, &mut addr_space)
 ├─ vfs::open(path) -> file
 ├─ file::read(0, ehdr_buf) -> parse Elf64_Ehdr
 ├─ for each PT_LOAD phdr:
 │     pages = (vaddr, memsz, flags) -> addr_space.map(...)
 │     file::read(offset, &mut pages[..filesz])
 │     zero pages[filesz..memsz]      # .bss
 ├─ stack = addr_space.map(STACK_TOP - 1MiB, RW|NX)
 ├─ build argv/envp/auxv on stack
 └─ return entry, rsp
```

## Process states (M11)

```
   spawn ──> Runnable ──> Running ──┬──> Blocked  (placeholder, M12)
                                    └──> Zombie   (exit)
```

`Runnable ↔ Running` is degenerate in M11 (single user task). M12 adds the queues.

## Memory layout per user process

```
0xFFFF_FFFF_FFFF_FFFF  +-------------------------------+
                       | kernel half (shared mapping)  |
0xFFFF_8000_0000_0000  +-------------------------------+
                       |              ...              |
0x0000_7FFF_FFFF_E000  +-------------------------------+
                       | user stack (1 MiB, grows down)|
0x0000_7FFF_FFEE_0000  +-------------------------------+
                       |              ...              |
0x0000_0000_0040_0000  +-------------------------------+
                       | ELF text/data (PT_LOAD)       |
0x0000_0000_0040_0000  +-------------------------------+
                       | guard / unmapped              |
0x0000_0000_0000_0000  +-------------------------------+
```

## Syscall numbers (M11 subset)

| `rax` | name        | args                              |
| ----- | ----------- | --------------------------------- |
| 0     | `read`      | `(fd, buf, count)`                |
| 1     | `write`     | `(fd, buf, count)`                |
| 2     | `openat`    | `(dirfd, path, flags, mode)`      |
| 3     | `close`     | `(fd)`                            |
| 9     | `mmap`      | `(addr, len, prot, flags, fd, off)` |
| 11    | `munmap`    | `(addr, len)`                     |
| 39    | `getpid`    | `()`                              |
| 60    | `exit`      | `(status)`                        |

## Failure paths

- Bad ELF (magic / arch / overflow) → `spawn_init` returns `Err`; kernel logs and falls back to in-kernel shell.
- `#PF` in user code → handler maps to SIGSEGV (M11: kill + log; full delivery in M12).
- `#PF` in `copy_from_user` → fixup sets `RAX = -EFAULT`, syscall returns cleanly.
- Triple fault from broken syscall stub → QEMU exits; runner reports panic via `isa-debug-exit`.
