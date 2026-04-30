# work25 — M25: Type-2 hypervisor (VMX/SVM, EPT, /dev/willo-vmx, virtio backends, willoc-boxes)

> Derived from `docs/idea.md` §18 (Virtualization & Containers).

## Goal

Stand up a KVM-class type-2 hypervisor inside the Willo kernel. Bootstrap **Intel VT-x / AMD SVM**, build per-vCPU **VMCS/VMCB** plumbing, walk **EPT/NPT** for nested paging, expose a `/dev/willo-vmx` ioctl surface intentionally shaped like Linux KVM, and ship a userspace **`willo-vmm`** library + a desktop **`willoc-boxes`** GUI to launch VMs. Running a small Linux guest (e.g. busybox initrd) is the v1 acceptance bar.

## Depends on

- **M11** — userspace + ioctl surface.
- **M13** — PCI + APIC (for IRQ injection).
- **M22** — virtio-gpu backend (shared with guest).
- **M39** — sandboxing profile for VMM process.

## Acceptance criteria

- [ ] CPUID feature detection: VMX/SVM enabled or graceful "not supported" message.
- [ ] `KVM_CREATE_VM`, `KVM_CREATE_VCPU`, `KVM_RUN`, `KVM_SET_REGS`, `KVM_GET_SREGS`, `KVM_IRQ_LINE` round-trip.
- [ ] EPT/NPT page faults handled; lazy population works.
- [ ] virtio-net, virtio-blk, virtio-console backends route guest traffic to host netstack/storage/console.
- [ ] `kernel/tests/vm_boot_linux.rs` boots a 1 MiB busybox initrd Linux guest; first userspace `init` prints "hello vm".
- [ ] `willoc-boxes` GUI launches a VM from a `.qcow2` (read-only first; CoW deferred).
- [ ] VM exit storm rate-limited; host CPU never pegged > 50% on idle guest.
- [ ] No host kernel oops across 1k VM create/destroy cycles.

## Task breakdown

### T1. CPU enablement — `kernel/src/virt/hwenable.rs` (new)
- CPUID checks; toggle `CR4.VMXE`; allocate VMXON region per-CPU; `vmxon`.
- AMD path symmetrical via `EFER.SVME` + `vmrun`.

### T2. VMCS/VMCB management — `kernel/src/virt/vmcs.rs`
- Per-vCPU VMCS; `vmwrite` config: pin/proc-based exec controls, exit/entry controls, EPT pointer, host state, guest state.
- Encapsulate VMX/SVM diff behind a `trait Vmm`.

### T3. EPT walker — `kernel/src/virt/ept.rs`
- 4-level paging mirroring guest physical → host physical.
- Lazy fill on EPT misconfiguration / misses.
- IOMMU passthrough deferred to a future M25.x.

### T4. Exit handler dispatch — `kernel/src/virt/exit.rs`
- `enum ExitReason { CPUID, MMIO, IO, HLT, EPTViolation, EPTMisconfig, ExternalInterrupt, ... }`.
- Default: re-enter guest. CPUID + MMIO/IO + HLT have host-side handlers.

### T5. ioctl surface — `kernel/src/virt/dev.rs`
- `/dev/willo-vmx` char dev; ioctls intentionally aligned to Linux KVM numbers.
- File descriptors per VM and per vCPU.

### T6. virtio backends — `kernel/src/virt/virtio/{net,blk,console}.rs`
- Implement device side of virtqueues; bridge to host netstack / block device / TTY.

### T7. userspace `willo-vmm` — `userspace/willo-vmm/`
- Idiomatic Rust client for `/dev/willo-vmx`.
- Boots from a kernel + initrd, or from a UEFI image.
- Configurable: vCPU count, RAM, virtio devices, net mode (user/bridged).

### T8. `willoc-boxes` GUI — `userspace/willoc-boxes/`
- Compositor client; lists VMs, edit specs, start/stop, view console (virtio-console -> framebuffer terminal).

### T9. §39 profile + audit
- VMM process runs under MAC profile; only cap is `/dev/willo-vmx`.
- Every `KVM_RUN` exit reason audited at debug level.

## New / modified files

| Path | Change |
| --- | --- |
| `kernel/src/virt/*` | **new** module |
| `kernel/src/main.rs` | wire `virt::detect_and_enable()` |
| `userspace/willo-vmm/` | **new** |
| `userspace/willoc-boxes/` | **new** |

## Tests to add

- `kernel/tests/vmx_enable.rs` — VMXON/off; no oops.
- `kernel/tests/vm_boot_linux.rs` — boot busybox guest; check first userspace print.
- `kernel/tests/ept_lazy.rs` — guest accesses unmapped page; host populates lazily.
- `kernel/tests/virtio_net_pingpong.rs` — guest ping → host bridge → reply.
- `userspace/willo-vmm/tests/cycle.rs` — 1k create/destroy cycles, no leak.

## Risks & open questions

- **VM escape** — catastrophic; default-off until external security audit. Opt-in via `/etc/willo/virt.toml`.
- **Nested virtualization** — explicit non-goal in M25; revisit later.
- **Live migration** — not in scope; document as future M25.x.
- **Vendor diff (Intel vs AMD)** — keep behind a `Vmm` trait; first-class Intel, AMD best-effort.
- **IOMMU passthrough** — deferred (USB/GPU passthrough is a M25.x topic).
- **Performance** — minimize VM exits with paravirtual virtio + APICv/AVIC; hardware-acceleration of nested paging required.
