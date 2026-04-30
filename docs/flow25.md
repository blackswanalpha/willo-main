# flow25 — M25: architecture & runtime flows (Hypervisor)

> Derived from `docs/idea.md` §18 and the work25 plan.

## Component map

```
        +-------------------+
        |  willoc-boxes GUI |
        +---------+---------+
                  |
                  v
        +-------------------+        +---------------------+
        |   willo-vmm lib   |<-----> | guest config (TOML) |
        +---------+---------+        +---------------------+
                  |  ioctl /dev/willo-vmx
                  v
        +----------------------------------------------+
        |  kernel virt:                                |
        |   hwenable | vmcs | ept | exit dispatch      |
        |   virtio backends: net, blk, console         |
        +-------+--------+--------+----------+---------+
                |        |        |          |
                v        v        v          v
            netstack  block    TTY       VMX/SVM hardware
            (M14)     (M13)
                                   |
                                   v
                              guest memory + vCPUs
```

## VM create / boot sequence

```
willoc-boxes "Run guest"
  -> willo-vmm: spawn process under §39 profile
  -> open /dev/willo-vmx
  -> ioctl KVM_CREATE_VM           -> kernel allocates Vm { ept, vcpus[], devices[] }
  -> mmap guest RAM region          -> EPT-mapped lazily
  -> ioctl KVM_CREATE_VCPU(0)       -> alloc VMCS for vcpu 0
  -> ioctl KVM_SET_SREGS(reset)     -> long-mode entry point per kernel cmdline
  -> spawn virtio-net backend       -> bridges to host wlan0/eth0
  -> ioctl KVM_RUN(0)               -> kernel: vmlaunch
       guest runs ...
       VM exit reason X
       host handler decides: emulate or return to userspace
       vmresume
  -> guest userspace `init` prints "hello vm" via virtio-console
```

## VM-exit dispatch

```
vmlaunch
  guest executes
  -> sensitive instr / EPT violation / IRQ
  -> VM exit
  -> kernel reads VMCS exit-reason / qualification
  -> match reason:
       CPUID              -> emulate (return host CPUID)
       MMIO/IO            -> route to virtio backend
       HLT                -> idle vcpu until IRQ
       EPTViolation       -> map missing page (lazy)
       EPTMisconfig       -> kill VM; report fault
       ExternalInterrupt  -> resume guest immediately
       Other              -> userspace VMM (KVM_RUN returns)
  -> vmresume
```

## EPT lazy fill

```
guest writes to GPA 0x100000 (unmapped)
  -> EPT violation exit
  -> handler:
        gpa = exit_qualification_gpa()
        host_page = guest_ram_alloc(gpa)
        ept::map(gpa, host_page, RWX)
  -> vmresume
  guest write succeeds; transparent
```

## virtio-net packet flow

```
guest sends packet:
  guest driver writes to virtq_avail
  rings doorbell → MMIO write → VM exit
  host virtio-net backend pulls descriptor
  copies bytes (or zero-copy via shared map)
  injects to host netstack -> bridge -> wlan0/eth0
  later: host receives reply, descriptor placed in virtq_used
  inject IRQ to guest (LAPIC virtual IRQ)
  guest driver reads
```

## VM destroy

```
willo-vmm exits OR ioctl KVM_DESTROY_VM
  -> for each vcpu: vmclear(vmcs)
  -> tear down virtio backends
  -> free EPT pagetables
  -> free guest RAM
  -> file refcount → 0 → vm dropped
no host kernel state retained
```

## Failure paths

- **VMX not supported by CPU** → ioctl returns `ENOSYS`; willoc-boxes shows "virtualization not available".
- **EPT misconfiguration** → guest killed; willo-vmm receives `KVM_RUN` ENOTSUP exit; user sees "guest crashed".
- **Triple fault in guest** → exit reason `TRIPLE_FAULT`; VM destroyed; UI shows guest BSOD-equivalent.
- **virtio-net buffer flood** → backend back-pressures (returns descriptors slowly); guest driver drops; counter increments.
- **Host OOM during EPT fill** → `ENOMEM` exit; guest sees host-as-hardware fault; user notified.
- **vCPU stuck in HLT for >60 s** → no-op; host idles vcpu thread; standard.

## Data structures

```rust
pub struct Vm {
    pub id: u32,
    pub ept: EptPaging,
    pub vcpus: Vec<VCpu>,
    pub devices: Vec<Box<dyn VirtioDevice>>,
    pub ram: Vec<RamSlot>,                  // sparse [gpa..gpa+len]
}

pub struct VCpu {
    pub id: u32,
    pub vmcs: PhysAddr,
    pub regs: GuestRegs,
    pub state: VCpuState,                   // Idle | Runnable | Blocked
}

pub enum ExitReason {
    Cpuid,
    Io { port: u16, dir: IoDir, size: u8 },
    Mmio { gpa: u64, size: u8, write: bool },
    Hlt,
    EptViolation { gpa: u64, gva: u64, write: bool, exec: bool },
    EptMisconfig { gpa: u64 },
    ExternalInterrupt(u8),
    TripleFault,
    Other(u32),
}

pub trait VirtioDevice: Send + Sync {
    fn handle_kick(&self, queue: u16);
    fn config_read(&self, off: usize, buf: &mut [u8]);
    fn config_write(&self, off: usize, buf: &[u8]);
}
```
