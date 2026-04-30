# work11 — M11: Userspace foundations (ring 3, syscall ABI, ELF, init+sh)

> Derived from `docs/idea.md` §4 (Kernel Core), §9 (Userspace & ABI).

## Goal

Cross the kernel/user boundary for the first time. Boot loads `/sbin/init` from FAT, ELF-maps it into a fresh address space, drops to ring 3, and runs a tiny userspace shell that talks back via `syscall`.

## Depends on

- **M10** — FAT32 write (needed to seed `/sbin/init` and `/bin/sh` into the data disk).

## Acceptance criteria

- [ ] GDT contains user code (idx 3), user data (idx 4), user32 code (idx 5).
- [ ] `EFER.SCE = 1`; `STAR`, `LSTAR`, `FMASK` MSRs configured.
- [ ] First 8 syscalls work: `write`, `read`, `exit`, `openat`, `close`, `mmap`, `munmap`, `getpid`.
- [ ] ELF64 static binary loaded, mapped, and executed.
- [ ] `/sbin/init` (Rust no_std bin) prints "hello from userspace" via `write(1, …)` and `exit(0)`.
- [ ] Kernel survives a userspace `#PF` — process killed, coredump stub written, kernel keeps running.
- [ ] `kernel/tests/ring3_jump.rs` confirms `CS` and `SS` reflect ring 3 in user code.

## Task breakdown

### T1. GDT user segments — `kernel/src/gdt.rs`
- Append `USER_CODE_64`, `USER_DATA`, `USER_CODE_32` selectors.
- Order matters for `STAR` MSR layout (kernel CS+8 = kernel SS, user CS+16 = user SS).

### T2. Syscall MSRs — `kernel/src/syscall.rs` (new)
- Set `IA32_EFER.SCE`.
- `STAR` = packed (KCODE, UCODE).
- `LSTAR` = address of `syscall_entry` asm stub.
- `FMASK` = clear `IF` and `DF` on entry.

### T3. Syscall entry stub — naked asm in `kernel/src/syscall.rs`
- `swapgs` → load kernel `gs` (per-CPU TLS).
- Save user `rsp` to per-CPU slot, load kernel `rsp`.
- Push regs, call `syscall_dispatch(rax, rdi, rsi, rdx, r10, r8, r9)`.
- Pop, restore user `rsp`, `swapgs`, `sysretq`.

### T4. Syscall dispatch table — `kernel/src/syscall.rs`
- Numbered, ABI-stable; reserve 0..511.
- Each handler returns `Result<u64, Errno>` mapped to `rax` (negative on error).

### T5. Per-process address space — `kernel/src/process.rs` (new)
- `Process { pid, addr_space, files, regs, state }`.
- `AddrSpace::new_user()` — fresh PML4 with kernel high half mapped, user half empty.
- Context switch swaps `cr3` and per-CPU TLS.

### T6. ELF loader — `kernel/src/elf.rs` (new)
- Parse `Elf64_Ehdr` + `PT_LOAD` program headers.
- Map each loadable segment with correct prot bits (NX on data).
- Set up user stack (anon mapping at top of user half, e.g. `0x0000_7fff_ffff_e000`).

### T7. Usercopy — `kernel/src/usercopy.rs` (new)
- `copy_from_user(usrptr, len) -> Result<Vec<u8>, Errno>`.
- `copy_to_user(usrptr, &[u8]) -> Result<(), Errno>`.
- `#PF` fixup: per-CPU "expected fault" flag → handler sets `RAX = -EFAULT` and returns.

### T8. First userspace program — `userspace/sh/`
- Tiny no_std `bin` crate; calls `write(1, …)` + `exit(0)` via inline `syscall`.
- Built into `target/x86_64-willo-user/release/sh`; QEMU runner copies to data disk as `/bin/sh`.

### T9. `/sbin/init` — `userspace/init/`
- For M11: minimal — just `write` + `exit`. Real `fork`/`exec` loop arrives in M12.

## New / modified files

| Path | Change |
| --- | --- |
| `kernel/src/gdt.rs` | + user selectors |
| `kernel/src/syscall.rs` | **new** (MSRs + dispatch + asm stub) |
| `kernel/src/process.rs` | **new** |
| `kernel/src/elf.rs` | **new** |
| `kernel/src/usercopy.rs` | **new** |
| `kernel/src/main.rs` | spawn init at boot |
| `userspace/sh/Cargo.toml`, `src/main.rs` | **new** |
| `userspace/init/Cargo.toml`, `src/main.rs` | **new** |
| `Cargo.toml` (root) | add `userspace/*` to workspace |
| `src/main.rs` (QEMU runner) | copy userspace ELFs into data disk |

## Tests to add

- `kernel/tests/syscall_write.rs` — load tiny ELF that prints; assert serial output.
- `kernel/tests/elf_hello_world.rs` — ELF loader integration.
- `kernel/tests/ring3_jump.rs` — verify `CS`/`SS` reflect ring 3.
- `kernel/tests/usercopy_fault.rs` — invalid pointer returns `EFAULT`; kernel survives.

## Risks & open questions

- **`swapgs` ordering** — easy to corrupt silently; write the stub once, freeze it.
- **`#PF` during `copy_from_user`** must not double-fault — fixup table or per-CPU "expected fault" flag.
- **32-bit compat (`int 0x80`)** — out of scope; only `syscall` for now.
- **ELF dynamic linking** — out of scope (M11 is static-only); deferred.
- **Spectre/Meltdown mitigations** — not in M11; revisit during §12 hardening.
