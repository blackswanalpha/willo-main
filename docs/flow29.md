# flow29 — M29: architecture & runtime flows (eBPF + perf tracing)

> Derived from `docs/idea.md` §19 and the work29 plan.

## Component map

```
        userspace
        +------------------+        +-------------------+
        |  willotrace CLI  |<------>| .bpf.o (ELF+BTF)  |
        +--------+---------+        +-------------------+
                 |  bpf() / perf_event_open()
                 v
        +-----------------------------------------------+
        |  kernel observ/ebpf:                          |
        |   isa | verifier | jit | interp | maps        |
        |   attach: tracepoint | kprobe | uprobe        |
        +---+--------+----------+--------------+--------+
            |        |          |              |
            v        v          v              v
        kernel    text patch  user text   per-CPU ringbuf
        statics                            (PMU samples)
```

## Load + verify + JIT

```
willotrace load prog.bpf.o
  -> parse ELF; extract sections (.text, .maps, .BTF)
  -> CO-RE: rewrite struct field offsets using kernel BTF
  -> bpf(BPF_MAP_CREATE, ...) for each map
  -> bpf(BPF_PROG_LOAD, prog_bytes, license, BTF_fd)
       -> kernel decode -> verifier
            walk CFG; abstract-interpret regs
            if reject: return -EINVAL with reason+pc
            if accept: -> JIT or interpreter
       -> attach point file_descriptor
  -> bpf(BPF_LINK_CREATE, prog_fd, target=tracepoint:sys_enter_write)
       -> attach::tracepoint::register(id, prog)
```

## Tracepoint runtime

```
syscall sys_write(...)
  -> tracepoint TP_sys_enter_write fires
       -> for prog in tracepoint_progs[id]:
            jit::run(prog, ctx)
            (or) interp::run(prog, ctx)
       -> prog increments map[pid] (hash map)
   -> normal sys_write continues
   (zero overhead if no prog attached; ~30 ns w/ JIT)
```

## Kprobe runtime

```
willotrace attach kprobe vfs_read
  -> kernel locates symbol; saves first byte; writes INT3
  -> when CPU hits INT3:
       -> #BP exception
       -> handler retrieves saved byte + pt_regs
       -> runs eBPF prog
       -> single-step the original instruction (using TF + #DB handler)
       -> resume
```

## Map access (hash)

```
prog: bpf_map_update_elem(&counts, &pid, &one, BPF_ANY)
  -> helper(map_fd, key_ptr, val_ptr, flags)
  -> verifier already checked types
  -> map.update(key, val)
   userspace: willotrace dump-map counts
        -> bpf(BPF_MAP_LOOKUP_ELEM, ...) per key
        -> prints (pid, count)
```

## PMU sampling flow

```
willotrace pmu-sample 99hz 1s
  -> perf_event_open(SAMPLE_FREQ=99, type=HARDWARE, config=CPU_CYCLES)
  -> kernel arms PMC; configures NMI on overflow
  -> NMI handler:
       record (cpu, rip, stack[..32])
       push to per-CPU ringbuf (lock-free SPSC)
  -> userspace reads ringbuf via mmap
  -> stack walk via DWARF (kernel) + frame pointers (user)
  -> flamegraph script collapses (folded -> svg)
```

## CO-RE relocation flow

```
prog references task_struct::pid
  -> compile-time: emits CORE relocation entry
  -> at load:
       look up "task_struct" in kernel BTF
       resolve "pid" field offset (e.g. 1024)
       patch instruction immediates (pid_off = 1024)
  -> JIT/interp executes correctly across kernel versions
```

## Failure paths

- **Verifier reject** → `bpf(BPF_PROG_LOAD)` returns `-EINVAL`; userspace prints reason + pc; never installed.
- **JIT emit unsupported insn** → fallback to interpreter; warning logged once.
- **Map full (BPF_ANY w/ no_replace)** → helper returns `E2BIG`; prog logic must handle.
- **Kprobe target symbol absent** → attach fails with `ENOENT`.
- **PMU NMI storm** (very high freq) → sampler self-throttles by halving freq; logged.
- **Ringbuf reader slow** → drop counter increments; not silent.

## Data structures

```rust
pub enum BpfReg {
    Scalar { min: i64, max: i64 },
    PtrToCtx,
    PtrToStack { off: i64 },
    PtrToMapVal { map_id: u32, off: i64, len: u32 },
    PtrToPacket { off: i64, end: i64 },
    Unknown,
}

pub struct BpfProg {
    pub insns: Box<[Insn]>,
    pub jit: Option<JitCode>,            // None => interpreter
    pub maps_used: Vec<MapId>,
    pub attach_kind: AttachKind,
    pub btf: Option<BtfHandle>,
}

pub enum MapKind {
    Hash, Array, PercpuArray, LruHash, LpmTrie, RingBuf,
}

pub struct PerfEvent {
    pub cpu: u8,
    pub kind: PerfKind,                  // HwCycles | HwInstr | SwCpuClock | ...
    pub freq_or_period: PerfRate,
    pub ringbuf: Arc<PerCpuRingBuf>,
}
```
