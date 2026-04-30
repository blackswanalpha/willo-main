# work29 — M29: eBPF + perf tracing (verifier, JIT, BTF, maps, kprobes/uprobes, willotrace, PMU)

> Derived from `docs/idea.md` §19 (Observability, Logging & Telemetry).

## Goal

Add a verified, JIT-compiled, in-kernel programmable tracing/observability surface. Define a Willo-eBPF ISA aligned to Linux eBPF, build a **path-sensitive verifier** (bounds, leaks, single spinlock, instruction count), an **x86-64 JIT** (interpreter fallback), the standard **map** types (hash/array/per-CPU/LRU/LPM/ringbuf), tracepoint + kprobe + uprobe attach points, **BTF** for CO-RE, and **PMU sampling** so `willotrace` can produce flamegraphs.

## Depends on

- **M16** — journal (rejection reasons + per-CPU buffers).
- **M11** — userspace + syscalls (`bpf`-class syscall surface).
- **M13** — APIC/MSI for PMU NMI delivery.

## Acceptance criteria

- [ ] Willo-eBPF ISA spec lives at `docs/spec/willo-ebpf.md` (numerically aligned to Linux eBPF where it matches).
- [ ] Verifier rejects unbounded loops, OOB pointer arith, leaks, multi-spinlock, and accepts a baseline of valid programs.
- [ ] x86-64 JIT compiles all in-tree test programs; interpreter fallback runs the same suite.
- [ ] Maps (hash, array, per-CPU array, LRU hash, LPM trie, ringbuf) round-trip under userspace test pressure.
- [ ] Attach points work: tracepoint (`sys_enter_write`), kprobe (`vfs_read`), uprobe (`libc::malloc`).
- [ ] BTF emitted from rustc DWARF; userspace `willotrace -e prog.bpf.o` loads via CO-RE.
- [ ] PMU sampling at 99 Hz produces a flamegraph showing the kernel hot path of a synthetic load.
- [ ] `kernel/tests/ebpf_basic.rs` runs a verified program counting `sys_write` calls.
- [ ] Verifier rejection logs include line numbers from BTF when available.

## Task breakdown

### T1. ISA + decoder — `kernel/src/observ/ebpf/isa.rs`
- 11 instruction classes (ALU/ALU64, JMP, LD/ST, MEM, atomic).
- Decoder produces a typed IR.

### T2. Verifier — `kernel/src/observ/ebpf/verifier.rs`
- Build CFG; abstract interpretation tracks each register's type (scalar with bounds | pointer to map_value, packet, stack, ctx).
- Constraints: ≤1M instructions, no unbounded loops (bounded loops since Linux 5.3 OK), bounds checks before deref, single spinlock.
- Output verdict + rejection reason w/ instruction index.

### T3. JIT — `kernel/src/observ/ebpf/jit_x86_64.rs`
- Per-bb code emission; register allocation 11 eBPF regs → x86-64.
- Helper-call calling convention matched to Willo kernel ABI.

### T4. Interpreter — `kernel/src/observ/ebpf/interp.rs`
- Reference implementation, used for maps/helpers and as JIT fallback.

### T5. Map types — `kernel/src/observ/ebpf/maps/`
- `hash`, `array`, `percpu_array`, `lru_hash`, `lpm_trie`, `ringbuf`.
- `bpf_map_lookup_elem` / `update_elem` / `delete_elem` helper triad.

### T6. Attach points — `kernel/src/observ/ebpf/attach/`
- Tracepoint table (kernel-emitted statics): tracepoint id → `Vec<BpfProg>`.
- Kprobe: text patcher inserting INT3; handler reads regs, runs prog; restores.
- Uprobe: same in user text; relies on M37 for ptrace-like semantics.

### T7. BTF — `kernel/src/observ/ebpf/btf.rs` + build script
- Emit BTF blob from rustc DWARF after kernel build; embed in kernel image.
- Userspace libbpf-class loader resolves CO-RE relocations against this BTF.

### T8. PMU sampling — `kernel/src/observ/perf.rs`
- `perf_event_open`-shaped syscall surface.
- Periodic NMI samples `RIP` (and stack via fp/dwarf walk) into per-CPU ringbuf.

### T9. `willotrace` CLI — `userspace/willotrace/`
- Loads `.bpf.o` ELFs; parses BTF; resolves relocations; submits via `bpf` syscall.
- Subcommands: `attach`, `detach`, `dump-map`, `pmu-sample`, `flamegraph`.

## New / modified files

| Path | Change |
| --- | --- |
| `kernel/src/observ/ebpf/*` | **new** module |
| `kernel/src/observ/perf.rs` | **new** |
| `kernel/src/syscall.rs` | + `bpf` + `perf_event_open` syscalls |
| `userspace/willotrace/` | **new** |
| `docs/spec/willo-ebpf.md` | **new** ISA spec |

## Tests to add

- `kernel/tests/ebpf_verifier_reject.rs` — vector of bad programs; each rejected with expected reason.
- `kernel/tests/ebpf_jit_vs_interp.rs` — every test program produces identical output via JIT and interpreter.
- `kernel/tests/ebpf_basic.rs` — count `sys_write` over 1k calls; map value matches.
- `kernel/tests/ebpf_kprobe_vfs_read.rs` — kprobe fires on real `vfs_read`.
- `kernel/tests/perf_sample_flamegraph.rs` — 99 Hz sampling for 1 s; flamegraph contains synthetic hot fn.

## Risks & open questions

- **Verifier conservatism** — surface clear rejection reasons; gate experimental relaxations behind a feature flag.
- **JIT correctness** — every program runs interpreter + JIT in tests; mismatch fails CI.
- **Kprobe text patching on SMP** — must use stop_machine-class quiescing; complicated; reuse §23 freeze plumbing.
- **PMU NMI safety** — sampler can run inside other locks; keep sampler lock-free; per-CPU ringbufs only.
- **BTF size** — embed compressed; gate behind a kernel feature flag if image bloats.
- **Helper count growth** — keep helpers small at first (≤32); document an extension policy.
