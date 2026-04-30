use core::cell::UnsafeCell;
use spin::Lazy;
use x86_64::VirtAddr;
use x86_64::structures::gdt::{Descriptor, DescriptorFlags, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

const KERNEL_RSP0_STACK_SIZE: usize = 4096 * 5;
static mut KERNEL_RSP0_STACK: [u8; KERNEL_RSP0_STACK_SIZE] = [0; KERNEL_RSP0_STACK_SIZE];

/// `TaskStateSegment` requires `&mut` to mutate, but we need to hand a
/// `&'static TaskStateSegment` to the GDT *and* be able to update fields like
/// `privilege_stack_table[0]` after init (e.g. per-process kernel-stack swap
/// in M12). Wrap in `UnsafeCell` and document the single-CPU invariant.
#[repr(transparent)]
struct TssCell(UnsafeCell<TaskStateSegment>);
unsafe impl Sync for TssCell {}

static TSS: Lazy<TssCell> = Lazy::new(|| {
    let mut tss = TaskStateSegment::new();
    tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
        const STACK_SIZE: usize = 4096 * 5;
        static mut STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];
        let stack_start = VirtAddr::from_ptr(&raw const STACK);
        stack_start + STACK_SIZE as u64
    };
    // Ring 0 stack used on CPL-changing interrupts/exceptions (e.g. #PF or
    // timer fired while CPU was in ring 3). syscall doesn't consult TSS — it
    // pivots via the GS-relative slot wired up in `syscall::init`.
    tss.privilege_stack_table[0] = {
        let stack_start = VirtAddr::from_ptr(&raw const KERNEL_RSP0_STACK);
        stack_start + KERNEL_RSP0_STACK_SIZE as u64
    };
    TssCell(UnsafeCell::new(tss))
});

pub struct Selectors {
    pub code_selector: SegmentSelector,
    pub data_selector: SegmentSelector,
    pub user_code_32_selector: SegmentSelector,
    pub user_data_selector: SegmentSelector,
    pub user_code_selector: SegmentSelector,
    pub tss_selector: SegmentSelector,
}

/// GDT layout — order is rigid because of SYSCALL/SYSRET MSR conventions.
///
/// SYSCALL: `CS = STAR[47:32]`, `SS = STAR[47:32] + 8`. So the kernel pair
/// must sit at `(N, N+1)`.
///
/// SYSRET to 64-bit: `CS = (STAR[63:48] + 16) | 3`, `SS = (STAR[63:48] + 8) | 3`.
/// So at the user STAR base, we must have `(user32_cs, user_ss, user_cs_64)`
/// at slots `(N, N+1, N+2)` — even though we never actually SYSRET to compat
/// mode, the 32-bit slot must be a valid descriptor for SYSRET indexing.
///
/// Resulting slot order: kernel_cs, kernel_ss, user_code_32, user_data,
/// user_code_64, tss (TSS descriptor occupies two 8-byte slots).
static GDT: Lazy<(GlobalDescriptorTable, Selectors)> = Lazy::new(|| {
    let mut gdt = GlobalDescriptorTable::new();
    let code_selector = gdt.append(Descriptor::kernel_code_segment());
    let data_selector = gdt.append(Descriptor::kernel_data_segment());
    let user_code_32_selector =
        gdt.append(Descriptor::UserSegment(DescriptorFlags::USER_CODE32.bits()));
    let user_data_selector = gdt.append(Descriptor::user_data_segment());
    let user_code_selector = gdt.append(Descriptor::user_code_segment());
    // SAFETY: the descriptor stores the TSS base address; subsequent mutations
    // through `set_kernel_stack` are visible to the CPU on the next ring
    // transition because the CPU re-reads TSS fields each time.
    let tss_ref: &'static TaskStateSegment = unsafe { &*TSS.0.get() };
    let tss_selector = gdt.append(Descriptor::tss_segment(tss_ref));
    (
        gdt,
        Selectors {
            code_selector,
            data_selector,
            user_code_32_selector,
            user_data_selector,
            user_code_selector,
            tss_selector,
        },
    )
});

/// Install our GDT and reload every segment register.
///
/// Why touch DS/ES/SS/FS/GS at all on x86_64? Because the bootloader handed us
/// a GDT in which `0x10` was a flat data segment, and on `iretq` the CPU
/// validates the popped SS selector against *our* freshly-loaded GDT — where
/// the same numeric selector now points into the TSS descriptor. That
/// mismatch raises a #GP that escalates to a double fault. Loading a real
/// kernel-data selector into all data registers makes `iretq` happy.
pub fn init() {
    use x86_64::instructions::segmentation::{CS, DS, ES, FS, GS, SS, Segment};
    use x86_64::instructions::tables::load_tss;

    GDT.0.load();
    unsafe {
        CS::set_reg(GDT.1.code_selector);
        SS::set_reg(GDT.1.data_selector);
        DS::set_reg(GDT.1.data_selector);
        ES::set_reg(GDT.1.data_selector);
        FS::set_reg(GDT.1.data_selector);
        GS::set_reg(GDT.1.data_selector);
        load_tss(GDT.1.tss_selector);
    }
}

/// Snapshot of the GDT selectors needed by syscall MSR setup and by tests
/// that build `iretq` frames into ring 3.
pub fn selectors() -> &'static Selectors {
    &GDT.1
}

/// Override the ring 0 stack pointer the CPU loads on a CPL-changing
/// interrupt/exception. The default set in `init` is fine for M11's single
/// kernel stack; per-process kernel stacks arrive in M12.
///
/// SAFETY: caller must guarantee no concurrent ring transition is in flight.
/// Single-CPU kernel makes this trivially true when called from boot before
/// the first `iretq` to ring 3.
pub unsafe fn set_kernel_stack(rsp: VirtAddr) {
    unsafe {
        (*TSS.0.get()).privilege_stack_table[0] = rsp;
    }
}
