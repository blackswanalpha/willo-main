# flow44 — M44: architecture & runtime flows (Pro-audio low-latency)

> Derived from `docs/idea.md` §11 and the work44 plan.

## Component map

```
        userspace
        +-------------+   +---------------+   +------------------+
        | DAW / synth |   | willojack     |   | willortkit       |
        | (JACK API)  |<->| graph + ports |<--| RT prio broker   |
        +-------------+   +-------+-------+   +------------------+
                                  |
                                  v
        +---------------------------------------------+
        | §40 willopipe pro-audio bridge / §18 mixer  |
        +-----------+-----------------+---------------+
                    |                 |
                    v                 v
              audio HW (HDA)    MIDI 2.0 sequencer (UMP)
                    |
                    v
          IRQs as kthreads (threadirq) at RT priority
```

## RT class basics

```
SchedClass::Rt(prio: 1..=99)
  preempts CFS unconditionally
  scheduling: per-CPU FIFO/RR queues
  budget: each cgroup gets cpu.rt.runtime_us within rt_period_us window
priority inheritance:
  if RT thread T blocks on mutex held by lower-prio L,
  L is bumped to T's priority until L releases
```

## willortkit grant flow

```
DAW calls libwillojack::ensure_rt(prio=70):
  -> D-Bus: willortkit.MakeRealtime { tid, prio=70 }
  -> rtkit checks:
       caller in `audio` group OR has CAP_SYS_NICE?
       prio within authorised range?
       process not too greedy (rate-limit)
  -> if ok: kernel sched_setscheduler(tid, SCHED_FIFO, prio=70)
  -> §39 audit log
return ok/fail
```

## willojack process cycle

```
buffer = 64 samples @ 48000 Hz -> 1.33 ms period
worker thread (RT prio):
  loop:
    wait for hardware DMA pointer to advance (poll or IRQ)
    pull `buffer` samples from inputs
    for node in topological order:
      node.process(inputs, outputs, buffer)
    push to output ringbuffer
    DMA hardware reads next period
```

## RT-preempt long kernel section

```
without PREEMPT_RT:
  kernel critical section runs ~150 µs uninterrupted -> jitter
with willo-rt:
  most spinlocks become rt_mutex (sleepable)
  cond_resched_rt() inserted at safe points
  RT thread can preempt mid-section -> jitter < 50 µs
```

## threadirq

```
default IRQ:
  hardware IRQ -> top-half handler in IRQ context -> bottom-half (softirq/tasklet)
threadirq:
  hardware IRQ -> top-half: ack + wake kthread irq/N
  kthread runs handler at its (RT) priority
  if audio kthread prio > network IRQ kthread prio: audio preempts
```

## MIDI 2.0 UMP

```
controller -> UMP packet (32-bit base, 64-bit if extended)
  message_type | group | status | params
sequencer routes by group + status
backwards-compat: UMP 1.0 ↔ MIDI 1.0 conversion via profile
property exchange: GET/SET device capabilities (e.g. patch list)
```

## Pro-audio mode toggle

```
Settings -> "Pro-audio" ON
  willortkit: enable group-based RT grant
  kernel: switch to willo-rt preemption mode (requires reboot OR live module swap)
  threadirq: move audio + USB IRQs to RT kthreads (prio 70/60)
  willojack: bring up; default 64 @ 48k
  PipeWire: enter pro-audio bridge mode
Settings -> "Pro-audio" OFF
  threadirq disabled; standard preempt; battery life recovers
```

## Failure paths

- **xrun (buffer underrun)** → willojack records xrun count; process emits silence frame; user sees notification on N+ xruns.
- **rtkit denial** → fall back to SCHED_OTHER; performance degraded; warning logged.
- **PI inversion still observed** → audit lock holders; surface suspect path in §29 perf trace.
- **RT runaway** (RT thread loops, refuses to yield) → cgroup `cpu.rt.runtime_us` budget kicks; RT-throttling kicks in (kernel logs).
- **Driver lacks threaded IRQ support** → falls back to softirq; latency budget at risk; warn in audio settings.

## Data structures

```rust
pub enum SchedClass {
    Idle,
    Cfs { weight: u32 },
    Rt { policy: RtPolicy, prio: u8 },     // 1..=99
}

pub struct ThreadIrq {
    pub irq: u32,
    pub kthread: KthreadId,
    pub prio: u8,                            // RT prio
    pub handler: fn(IrqContext) -> IrqReturn,
}

pub struct JackGraph {
    pub nodes: Vec<JackNode>,
    pub edges: Vec<(PortId, PortId)>,
    pub topo_order: Vec<NodeId>,
    pub buffer_size: u32,                    // 32..=8192
    pub sample_rate: u32,                    // 44100 or 48000 typical
}

pub struct UmpPacket {
    pub mt: u8,                              // message type 0..=15
    pub group: u8,                           // 0..=15
    pub status: u8,
    pub data: [u8; 14],                      // up to 128-bit UMP
}
```
