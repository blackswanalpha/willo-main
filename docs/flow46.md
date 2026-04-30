# flow46 — M46: architecture & runtime flows (Memory hardening v2)

> Derived from `docs/idea.md` §5 and the work46 plan.

## Component map

```
        +------------------------+
        |  boot: KASLR pick      |
        +-----------+------------+
                    |
                    v
        +-----------------------------------+
        | running kernel hardening overlay  |
        |  SMEP/SMAP | CET IBT/SS | CFI     |
        |  canaries  | slab rand | KFENCE   |
        +-----------+----------+------------+
                    |          |
                    v          v
              fault handlers   telemetry counters -> §16/§36
              (#PF, #CP)
```

## KASLR offset

```
boot stage:
  rdrand -> 64-bit random
  offset = (rand & ALIGN_MASK) within KASLR_RANGE
  relocate kernel image to (KERNEL_BASE_DEFAULT + offset)
  patch all absolute relocs
publish offset in boot_params for tools (perf, gdb-stub)
```

## SMEP / SMAP fault path

```
ring 0 instr fetched from a user page (e.g. ROP attempt to user code):
  CPU raises #PF with code SMEP
  kernel handler:
    if expected (via copy_to_user fixup): return -EFAULT
    else: oops; coredump; killed
SMAP:
  ring 0 reads/writes user page outside STAC region:
    CPU raises #PF SMAP
    treated like SMEP (oops)
copy_to_user:
  stac()
  *user_ptr = val
  clac()
```

## CET IBT

```
indirect call: rip set to target T
  CPU checks first byte of T
  if T[0..4] != 'endbr64' opcode: #CP exception
kernel handler:
  oops + dump (telemetry++)
  unless §29 eBPF JIT site: emit endbr at every entrypoint
```

## CET Shadow Stack

```
call instruction:
  push return addr -> normal stack
  push return addr -> shadow stack (SSP MSR)
ret:
  pop normal stack -> rip
  pop shadow stack -> compare with rip
  if mismatch: #CP exception
signal handling:
  save SSP into signal frame
  restore SSP on sigreturn
fork: shadow stack copied per task
```

## CFI

```
fn dispatch(handler: fn(u64)->u64) {
    handler(42);  // indirect call
}
compiler:
  emit type-id for `fn(u64)->u64`
  before call: cmp type-id at handler vs expected
  if mismatch: __cfi_check fault
allows attackers to overwrite handler ptr with arbitrary fn -> caught at call time
```

## Stack canary

```
fn foo(... heavy stack) {
    canary = gs:0x28
    place at end of stack frame
    ... function body ...
    if (canary != gs:0x28) __stack_chk_fail()
    return
}
__stack_chk_fail -> oops; coredump; killed
```

## Slab freelist randomization

```
slab_init(cache):
  build freelist [0, 1, 2, ..., n-1]
  fisher-yates shuffle with per-slab seed
slab_alloc(cache):
  pop next freelist index (random order)
  return obj
```

## KFENCE detection

```
allocator entry:
  if rand() % SAMPLE == 0:
     allocate from KFENCE pool (one obj per page; surrounding pages NOT_PRESENT)
on access:
  if access to NOT_PRESENT: #PF
  KFENCE handler:
     report OOB or UAF with allocator + access stack
     §16 journal: KFENCE event
```

## Failure paths

- **Pre-CET CPU** → CET features disabled; counters report "n/a"; no degradation in functionality.
- **`copy_from_user` malicious ptr** → SMAP #PF; expected-fault flag; -EFAULT returned.
- **Hijacked indirect call** → IBT #CP; oops + telemetry.
- **ROP gadget chain** → return mismatch on shadow stack; #CP; oops.
- **OOB heap access (sampled)** → KFENCE #PF; full report + immediate kill.

## Data structures

```rust
pub struct KaslrInfo {
    pub kernel_base: VirtAddr,
    pub offset: u64,
    pub fg_seed: Option<u64>,                // FG-KASLR per-fn seed
}

pub struct CetCtx {
    pub enabled: bool,
    pub ibt: bool,
    pub shadow_stack: bool,
    pub ssp: VirtAddr,                       // per-task
}

pub struct KfenceSlot {
    pub idx: u32,
    pub obj_addr: VirtAddr,
    pub size: usize,
    pub alloc_stack: ShortStack,
    pub free_stack: Option<ShortStack>,
    pub state: KfenceState,                  // Free | Allocated | Quarantine
}

pub struct HardeningTelemetry {
    pub ibt_faults: AtomicU64,
    pub ssp_faults: AtomicU64,
    pub smep_faults: AtomicU64,
    pub smap_faults: AtomicU64,
    pub cfi_faults: AtomicU64,
    pub canary_faults: AtomicU64,
    pub kfence_oob: AtomicU64,
    pub kfence_uaf: AtomicU64,
}
```
