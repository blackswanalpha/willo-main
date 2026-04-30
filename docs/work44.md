# work44 — M44: Pro-audio low-latency (RT scheduling, JACK-class server, MIDI 2.0)

> Derived from `docs/idea.md` §11 (Multimedia — pro-audio stretch).

## Goal

Make Willo viable for music production. Add a **PREEMPT_RT-style** preemption mode for the kernel, **SCHED_FIFO** real-time scheduling, audio thread RT priority via an **rtkit-class** broker, a **JACK 2-class** audio server (`willojack`) for sub-3 ms round-trip, and **MIDI 2.0** + Universal MIDI Packets in the kernel sequencer (extending §40).

## Depends on

- **M12** — preemptive scheduler (gains RT class).
- **M18** — audio core (HDA driver gains low-latency path).
- **M40** — MIDI sequencer (extended to MIDI 2.0).
- **M39** — sandbox + RT broker authority.

## Acceptance criteria

- [ ] `SCHED_FIFO` (priorities 1..99) and `SCHED_RR` schedulers implemented; bound by `kernel.sched_rt_runtime_us`-class budget.
- [ ] `willortkit` daemon grants RT priority to authorised processes (PolKit-class).
- [ ] `willojack` server runs at 64-sample @ 48 kHz with measured round-trip < 3 ms on QEMU virtio-sound.
- [ ] PREEMPT_RT-style mode reduces worst-case scheduling latency to ≤ 50 µs (vs ≤ 200 µs for default).
- [ ] threadirq mode: IRQ handlers run as kthreads with configurable priority.
- [ ] MIDI 2.0 / UMP: Universal MIDI Packet codec; backwards-compatible with MIDI 1.0.
- [ ] `kernel/tests/sched_fifo_jitter.rs` measures jitter; passes a target threshold.
- [ ] `userspace/willojack/tests/roundtrip_lat.rs` passes < 3 ms.

## Task breakdown

### T1. RT scheduling — `kernel/src/sched/rt.rs`
- Add `SchedClass::Rt(prio)`; runqueue per CPU; preempts CFS at all RT priorities.
- Inheritance for priority inversion (PI futexes).
- Budget: `rt_runtime_us` / `rt_period_us`; cgroup v2 `cpu.rt.runtime_us`.

### T2. PREEMPT_RT-style mode — `kernel/src/sched/preempt.rs`
- Convert spinlocks to mutexes where safe; RT-preemption points in long sections.
- Behind a kernel feature flag (`willo-rt`).

### T3. threadirq — `kernel/src/interrupts/threaded.rs`
- Move IRQ handlers to dedicated kthreads (per IRQ); driver opts in.
- IRQ thread priority surfaces under `/proc/irq/<n>/priority`.

### T4. willortkit — `userspace/willortkit/`
- D-Bus `org.willo.RealTimeKit`-class.
- Authorises RT priorities per process by group/uid + ulimit; logs to §39 audit.

### T5. willojack server — `userspace/willojack/`
- Graph node + port model; lock-free SPSC ringbuffers between nodes.
- Audio backend = §18 (low-latency path); MIDI = §40 sequencer.
- JACK API compat shim (`libwillojack`) so existing JACK clients link.

### T6. PipeWire pro-audio bridge — `userspace/willopipe/proaudio.rs`
- §40 daemon gains a "pro-audio" mode that exposes JACK-style nodes over §18.

### T7. MIDI 2.0 / UMP — `kernel/src/media/midi2.rs`, `userspace/willomidi/ump.rs`
- 32/64-bit Universal MIDI Packets; profile config + property exchange.

### T8. Latency tuning settings — `userspace/willoc-settings/plugins/audio.rs`
- Buffer size + sample rate selectors; "Pro-audio" toggle that flips PREEMPT_RT, threadirq, RT priorities.

## New / modified files

| Path | Change |
| --- | --- |
| `kernel/src/sched/rt.rs` | **new** |
| `kernel/src/sched/preempt.rs` | **new** RT-preempt mode |
| `kernel/src/interrupts/threaded.rs` | **new** threadirq |
| `kernel/src/media/midi2.rs` | **new** UMP |
| `kernel/src/cgroup/cpu_rt.rs` | RT budget controller |
| `userspace/willortkit/` | **new** |
| `userspace/willojack/` | **new** |
| `userspace/willopipe/proaudio.rs` | **new** mode |
| `userspace/willoc-settings/plugins/audio.rs` | **new** plugin |

## Tests to add

- `kernel/tests/sched_fifo_jitter.rs` — RT thread under load; jitter < N µs.
- `kernel/tests/preempt_rt_scope.rs` — long kernel-side op preempts cleanly.
- `userspace/willojack/tests/roundtrip_lat.rs` — RT round-trip < 3 ms.
- `userspace/willortkit/tests/grant_revoke.rs` — only authorised pid gets RT prio.
- `kernel/tests/midi2_ump_roundtrip.rs` — UMP encode/decode.

## Risks & open questions

- **Determinism vs throughput** — RT mode taxes throughput; expose toggle and profile each.
- **Priority inversion** — PI futexes mandatory; integration test with chains 3+ deep.
- **rtkit policy** — too lax = DoS; too strict = unusable. Default: `audio` group + soft-limit.
- **Driver participation** — every audio-relevant driver must support threaded IRQs (HDA, USB MSC for some interfaces); track gaps.
- **Power vs perf** — RT mode disables some C-states; document UX impact on §23 battery life.
- **MIDI 2.0 device coverage** — UMP support is patchy in 2026 hardware; ship MIDI 1.0 fallback always.
