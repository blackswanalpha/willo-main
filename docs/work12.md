# work12 — M12: Preemptive scheduler, processes/threads, fork/exec/wait, signals, IPC

> Derived from `docs/idea.md` §4 (Kernel Core).

## Goal

Move from "one userspace task that exits" (M11) to a real multi-process kernel: APIC-driven preemption, `fork`/`exec`/`wait`, signals, and the simplest IPC primitives (`pipe`, UNIX socket, shared memory).

## Depends on

- **M11** — ring 3, syscall ABI, ELF loader, per-process address spaces.

## Acceptance criteria

- [ ] APIC timer drives a 100 Hz preemption tick.
- [ ] Round-robin scheduler with run queue + sleep queue.
- [ ] `fork()` creates a CoW copy of the parent address space.
- [ ] `execve()` replaces the program image and resets stack/argv/envp.
- [ ] `waitpid()` reaps zombies; `SIGCHLD` delivered to parent.
- [ ] Signals: `SIGTERM`, `SIGKILL`, `SIGCHLD`, `SIGINT`, `SIGSEGV` deliverable.
- [ ] `pipe()` works between two cooperating user processes.
- [ ] `UNIX_SOCK` (datagram + stream) round-trip works in-process pair.
- [ ] `shm_open` + `mmap` shares a page across two processes.

## Task breakdown

### T1. APIC timer — `kernel/src/interrupts.rs`, `kernel/src/apic.rs` (new)
- Disable 8259 PIC; bring up local APIC with TPR=0.
- Calibrate APIC timer against PIT once; program periodic 10 ms.
- Wire vector 0x40 → `scheduler::tick`.

### T2. Scheduler — `kernel/src/sched.rs` (new)
- Per-CPU run queue (just one CPU until M15).
- `Process::Runnable | Running | Blocked(reason) | Zombie(exit_code)`.
- `schedule()` picks next, swaps `cr3`, restores regs, `iretq`.
- `yield_now()` reusable in syscall handlers when blocking.

### T3. `fork` — `kernel/src/process.rs`, `kernel/src/sched.rs`
- Clone PCB, file table (refcount), addr space.
- CoW: clear `RW` bit on every user page in both parent and child; `#PF` handler clones page on first write.
- Return `pid` to parent, `0` to child.

### T4. `execve` — `kernel/src/process.rs`
- Tear down old user mappings (keep PCB).
- ELF-load new image (reuse M11 loader).
- Rebuild stack with new argv/envp.

### T5. `wait`/`exit` — `kernel/src/process.rs`
- `exit(code)` → `Zombie(code)` + send `SIGCHLD` to parent + wake any `wait` waiter.
- `waitpid(pid, opts)` blocks on a `WaitQueue`; reaps PCB on success.

### T6. Signals — `kernel/src/signal.rs` (new)
- Per-process pending mask + handler table.
- Delivery on syscall return: build sigframe on user stack, set `RIP = handler`, return; `sigreturn` syscall restores.
- Default actions: `SIGKILL`/`SIGSEGV` terminate; `SIGCHLD` ignored by default.

### T7. Pipes — `kernel/src/ipc/pipe.rs` (new)
- Bounded ring buffer (4 KiB), reader/writer wait queues.
- `pipe()` syscall returns `[rfd, wfd]`.

### T8. UNIX sockets — `kernel/src/ipc/usock.rs` (new)
- Datagram + stream over an in-kernel ring; bind to a `/run` path resolved through VFS.

### T9. Shared memory — `kernel/src/ipc/shm.rs` (new)
- `shm_open(name, flags)` returns fd backed by anon pages held in a global table.
- `mmap` on that fd shares the same physical pages between processes.

### T10. Init upgrade — `userspace/init/`
- Real `init`: `fork` + `exec` `/bin/sh`; on `SIGCHLD`, `waitpid` and respawn.

## New / modified files

| Path | Change |
| --- | --- |
| `kernel/src/apic.rs` | **new** |
| `kernel/src/sched.rs` | **new** |
| `kernel/src/signal.rs` | **new** |
| `kernel/src/ipc/pipe.rs` | **new** |
| `kernel/src/ipc/usock.rs` | **new** |
| `kernel/src/ipc/shm.rs` | **new** |
| `kernel/src/process.rs` | + fork/exec/wait/exit |
| `kernel/src/interrupts.rs` | switch from PIC → APIC for timer |
| `kernel/src/syscall.rs` | + new syscall numbers |
| `userspace/init/src/main.rs` | real init loop |

## Tests to add

- `kernel/tests/preempt_yield.rs` — two CPU-bound tasks make progress.
- `kernel/tests/fork_exec_wait.rs` — child execs, parent waits, exit code matches.
- `kernel/tests/pipe_pair.rs` — fork; parent writes, child reads.
- `kernel/tests/signal_kill.rs` — `SIGKILL` reaps target.
- `kernel/tests/shm_share.rs` — two processes see the same memory.

## Risks & open questions

- **CoW under SMP** — single-CPU until M15, so no atomic-RMW needed yet.
- **Signal delivery races** — deliver only on syscall return / interrupt return, never mid-syscall.
- **Reentrant scheduler** — disable preemption while holding scheduler locks.
- **Pipe deadlock** — bounded buffer + blocking semantics; document behaviour.
