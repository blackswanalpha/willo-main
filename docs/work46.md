# work46 — M46: Memory hardening v2 (KASLR, SMEP/SMAP, CET IBT + Shadow Stack, CFI, KFENCE)

> Derived from `docs/idea.md` §5 (Memory Management — KASLR, SMEP/SMAP, W^X, stack guards, slab + buddy + OOM).

## Goal

Modernise Willo's exploit-mitigation surface. Land **KASLR** (kernel base randomization, optional FG-KASLR for functions), **SMEP/SMAP** enforcement, **CET — IBT (Indirect Branch Tracking) + Shadow Stack**, **CFI** (Clang-style kernel CFI for indirect calls), **kernel stack canaries**, **slab freelist randomization**, and a **KFENCE**-class probabilistic out-of-bounds / use-after-free detector. Wire to §29's PMU for sampling-based attack telemetry.

## Depends on

- **M5/M13** — paging + boot.
- **M11** — process model (CET shadow stack interacts with signal handling).
- **M16** — journal (denial events).
- **M29** — PMU sampling.

## Acceptance criteria

- [ ] KASLR randomises kernel base each boot; offset stored in `boot_params`; symbols in `dmesg` reflect offset.
- [ ] SMEP + SMAP bits set in CR4 on every CPU; userspace exec from kernel page faults; kernel access to user page faults inside `copy_to/from_user` paths uses STAC/CLAC.
- [ ] CET IBT: every indirect-call target prefixed with `endbr64`; missing `endbr64` faults `#CP`.
- [ ] CET Shadow Stack enabled in ring 0 + ring 3; signal frames save/restore SSP correctly.
- [ ] Clang/Rust CFI integrated for indirect calls; mismatched type triggers fault.
- [ ] Kernel stack canary on every function with stack frame > 8 B.
- [ ] Slab freelist randomized per slab.
- [ ] KFENCE pool detects a planted UAF in test.
- [ ] `kernel/tests/cet_ibt.rs`, `kernel/tests/cet_ssp.rs`, `kernel/tests/kfence_uaf.rs`, `kernel/tests/cfi_mismatch.rs` pass.

## Task breakdown

### T1. KASLR — `kernel/src/boot/kaslr.rs`
- Pick offset from RDRAND/RDSEED at boot; relocate kernel image.
- Optional FG-KASLR (per-function shuffle) behind feature flag.

### T2. SMEP/SMAP — `kernel/src/cpu/smap.rs`
- Set CR4.SMEP + CR4.SMAP on each CPU.
- `copy_to_user` / `copy_from_user` wrap `STAC`/`CLAC`; per-CPU "expected fault" flag for fixups.

### T3. CET IBT + Shadow Stack — `kernel/src/cpu/cet.rs`
- Compile kernel with `-fcf-protection=full` (or Rust equivalent).
- Detect CET via CPUID; enable via `MSR_IA32_U_CET`/`MSR_IA32_S_CET`.
- Per-task SSP allocation; signal frame save/restore.

### T4. CFI — build flags + `kernel/src/cfi.rs`
- Clang/Rust CFI (sanitize) for indirect call type checks.
- Trampolines per call signature.

### T5. Stack canary — `kernel/src/cpu/canary.rs`
- Per-CPU random canary at `gs:0x28` (Linux convention).
- `-fstack-protector-strong` for kernel + userspace.

### T6. Slab hardening — `kernel/src/mm/slab.rs`
- Freelist randomization per slab (Fisher-Yates at slab init).
- Hardened freelist pointer (XOR with cookie).

### T7. KFENCE — `kernel/src/mm/kfence.rs`
- Reserved guarded pool (default 256 slots × 4 KiB).
- Sampler picks small fraction of allocations; pages around object are NOT_PRESENT.
- Reports OOB / UAF via §16 journal.

### T8. Telemetry hooks — `kernel/src/security/telemetry.rs`
- Counters for IBT faults, SMEP faults, KFENCE detections; surfaced via §36 system monitor.

## New / modified files

| Path | Change |
| --- | --- |
| `kernel/src/boot/kaslr.rs` | **new** |
| `kernel/src/cpu/{smap,cet,canary}.rs` | **new** |
| `kernel/src/cfi.rs` | **new** |
| `kernel/src/mm/{slab,kfence}.rs` | extend / **new** |
| `kernel/src/security/telemetry.rs` | **new** |
| `Cargo.toml` (kernel) | + cfi + stack-protector flags |
| build scripts | KASLR relocation post-link step |

## Tests to add

- `kernel/tests/cet_ibt.rs` — call target without `endbr64` faults.
- `kernel/tests/cet_ssp.rs` — return mismatch faults; signal frame round-trip.
- `kernel/tests/cfi_mismatch.rs` — signature mismatch faults.
- `kernel/tests/kfence_uaf.rs` — UAF detected; offending stack captured.
- `kernel/tests/slab_freelist_random.rs` — two slabs of same kind have different freelist orders.

## Risks & open questions

- **CET hardware coverage** — older CPUs lack CET; gate via CPUID; degrade gracefully.
- **JIT W^X conflict** (§24) — Shadow Stack + JIT requires explicit signal trampoline alteration; document.
- **CFI transitive cost** — every indirect call gets a type-check; benchmark hot path; opt-out attribute for tight inner loops.
- **KFENCE noise** — sampler may miss bugs; complement with KASAN in CI builds (heavier).
- **Boot time KASLR** — randomization ≤ 1 ms; acceptable.
- **eBPF interaction (§29)** — CFI may reject JIT-emitted calls; emit endbr + register types per program.
