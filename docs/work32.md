# work32 — M32: Developer tooling (GDB stub, willo-strace, crash-dump symbolicator, willo-doc)

> Derived from `docs/idea.md` §17 (Developer Tooling).

## Goal

Make Willo a great place to debug Willo. Land a kernel **GDB Remote Serial Protocol** stub over UART/virtio-console, a userspace **`willo-strace`** syscall tracer (built on §29 uprobes), a **crash-dump** pipeline that captures kernel oops + userspace coredumps and produces symbolicated stacks, and a `willo-doc` mdbook-class documentation tool.

## Depends on

- **M11** — process model (coredumps).
- **M16** — system journal (oops + coredump metadata).
- **M29** — eBPF/uprobes (`willo-strace` attach).
- **M33** (optional) — self-hosted toolchain (`willo-doc` runs on Willo).

## Acceptance criteria

- [ ] `gdb -ex 'target remote :1234'` connects to a running QEMU Willo and can read kernel registers/memory, set breakpoints, single-step.
- [ ] `willo-strace ls` lists every syscall + arg + return value with symbolic syscall names and `errno` decode.
- [ ] Kernel oops produces a binary minidump on a panic disk (`/var/crash/` or serial-dump for QEMU); symbolicator turns it into human stack frames.
- [ ] Userspace SIGSEGV produces a coredump under `/var/crash/` with the same minidump format.
- [ ] `willo-doc serve` renders the `docs/` tree as a navigable site (mdbook-class).
- [ ] `kernel/tests/gdb_stub_breakpoint.rs` confirms breakpoint + continue + step.
- [ ] `kernel/tests/strace_basic.rs` verifies a known syscall sequence.
- [ ] `kernel/tests/crash_minidump.rs` triggers a controlled oops; minidump contains expected stack.

## Task breakdown

### T1. GDB RSP stub — `kernel/src/debug/gdb.rs`
- Per-CPU halt on `INT3` / single-step (`#DB`).
- `getDebugChar`/`putDebugChar` over UART (default) or virtio-console.
- Packet handlers: `g`/`G`, `m`/`M`, `c`, `s`, `Z0-4`, `vCont`, `qSupported`, `qXfer:features:read`.

### T2. SMP halt — `kernel/src/debug/halt_all.rs`
- IPI to other CPUs to halt at safe point; resume via IPI.

### T3. `willo-strace` — `userspace/willo-strace/`
- Attach eBPF uprobes to `syscall` entry/exit (M29).
- Decode args using a per-syscall arg-format table.
- Output one line per call: `n: openat(AT_FDCWD, "/etc/passwd", O_RDONLY) = 5`.

### T4. Coredump writer — `kernel/src/debug/coredump.rs`
- On userspace SIGSEGV / SIGABRT (or kernel oops), write minidump:
  - module list (with build-ids), thread regs, stack (≤256 KiB/thread), VMA map, signal info.
- Hash + ID + timestamp filename; placed in `/var/crash/`.

### T5. Symbolicator — `userspace/willo-symbolicate/`
- Reads minidump; resolves frames via build-id-keyed DWARF/symtab files in `/usr/lib/debug/`.
- Output JSON stacks + human text; integrate with §36 system monitor's "Crashes" pane.

### T6. `willo-doc` — `userspace/willo-doc/`
- mdbook-class: walk `docs/`, index, search; runnable on Willo (depends on M33 ideally).

### T7. Crash report UX — `userspace/willoc-crash/`
- Compositor client: list recent crashes, view symbolicated trace, opt-in upload to vendor URL (off by default).

## New / modified files

| Path | Change |
| --- | --- |
| `kernel/src/debug/gdb.rs` | **new** |
| `kernel/src/debug/halt_all.rs` | **new** |
| `kernel/src/debug/coredump.rs` | **new** |
| `kernel/src/main.rs` | wire `debug::init()` (gated `--features debug-stub`) |
| `userspace/willo-strace/` | **new** |
| `userspace/willo-symbolicate/` | **new** |
| `userspace/willo-doc/` | **new** |
| `userspace/willoc-crash/` | **new** |

## Tests to add

- `kernel/tests/gdb_stub_breakpoint.rs` — RSP packet round-trip; breakpoint hit; resume.
- `kernel/tests/crash_minidump.rs` — controlled oops; minidump contents validated.
- `kernel/tests/strace_basic.rs` — uprobe-driven trace of a known syscall sequence.
- `userspace/willo-symbolicate/tests/resolve_dwarf.rs` — frame addr → file:line via DWARF.

## Risks & open questions

- **GDB stub stalls every CPU** — clearly mark `--features debug-stub` for non-prod kernels; never ship in default image.
- **Symbol files distribution** — `-debuginfo` `.willo` packages parallel runtime packages; large; document space cost.
- **Minidump format stability** — pin a Willo-native format with a versioned header; document under `docs/spec/minidump.md`.
- **strace overhead** — uprobes are cheaper than ptrace but still measurable; document for benchmarking.
- **Privacy** — coredump contents may include secrets; uploading is opt-in only with explicit warning.
- **Self-hosting `willo-doc`** — until M33, runs cross-compiled on Linux; document this dual-build path.
