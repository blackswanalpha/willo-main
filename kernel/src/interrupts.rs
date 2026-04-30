use crate::gdt::DOUBLE_FAULT_IST_INDEX;
use crate::{QemuExitCode, exit_qemu, serial_println};
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use pic8259::ChainedPics;
use spin::{Lazy, Mutex};
use x86_64::PrivilegeLevel;
use x86_64::registers::control::Cr2;
use x86_64::structures::idt::{
    InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode,
};

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

pub static PICS: Mutex<ChainedPics> =
    Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum Irq {
    Timer = PIC_1_OFFSET,
    Keyboard,
}

impl Irq {
    fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Number of timer ticks observed since boot (set by the timer IRQ).
/// Tests can spin until this advances to prove that hardware interrupts fire.
pub static TIMER_TICKS: AtomicU64 = AtomicU64::new(0);

/// Snapshot the timer-tick counter. Useful as a coarse uptime source.
pub fn ticks() -> u64 {
    TIMER_TICKS.load(Ordering::Relaxed)
}

/// If a kernel-side `#PF` is expected (set by `usercopy::with_fixup`), the
/// page-fault handler rewrites the saved RIP to this address so `iretq`
/// resumes at the fixup label instead of replaying the faulting instruction.
/// Zero means "no fixup expected — kernel #PF is fatal".
pub static EXPECTED_FAULT_FIXUP: AtomicUsize = AtomicUsize::new(0);

/// Hook invoked by the breakpoint handler before the default log. Tests use
/// this to capture the saved interrupt frame (e.g. CS/SS to confirm ring 3)
/// without registering an entirely separate IDT.
pub static BP_HOOK: spin::Once<fn(&InterruptStackFrame) -> !> = spin::Once::new();

static IDT: Lazy<InterruptDescriptorTable> = Lazy::new(|| {
    let mut idt = InterruptDescriptorTable::new();
    // BP gate DPL=3 so INT3 (the one-byte software-breakpoint opcode) works
    // from ring 3 — needed by tests that confirm a successful ring transition
    // and by future userspace debuggers. The handler itself is harmless.
    idt.breakpoint
        .set_handler_fn(breakpoint_handler)
        .set_privilege_level(PrivilegeLevel::Ring3);
    unsafe {
        idt.double_fault
            .set_handler_fn(double_fault_handler)
            .set_stack_index(DOUBLE_FAULT_IST_INDEX);
    }
    idt.page_fault.set_handler_fn(page_fault_handler);
    idt.general_protection_fault.set_handler_fn(gp_handler);
    idt.invalid_opcode.set_handler_fn(ud_handler);
    idt[Irq::Timer.as_u8()].set_handler_fn(timer_handler);
    idt[Irq::Keyboard.as_u8()].set_handler_fn(keyboard_handler);
    idt
});

/// Load the IDT, remap and unmask the legacy PIC, then `sti`.
pub fn init() {
    IDT.load();
    unsafe { PICS.lock().initialize() };
    x86_64::instructions::interrupts::enable();
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    if let Some(hook) = BP_HOOK.get() {
        hook(&stack_frame);
    }
    serial_println!("EXCEPTION: BREAKPOINT (rip recorded in interrupt frame)");
}

extern "x86-interrupt" fn double_fault_handler(
    _stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    serial_println!("EXCEPTION: DOUBLE FAULT");
    exit_qemu(QemuExitCode::Failed);
}

/// Two distinct paths:
///
/// 1. `#PF` while CPL=3 (user code dereferenced something bad). In M11 we have
///    no process-kill machinery yet — log and halt. M12 will deliver SIGSEGV
///    and tear down the process.
/// 2. `#PF` while CPL=0. If `EXPECTED_FAULT_FIXUP` is non-zero, this is a
///    `copy_from_user` / `copy_to_user` walking a bad user pointer; rewrite
///    the saved RIP so `iretq` resumes at the fixup label inside usercopy.
///    Otherwise it's a real kernel bug — fail the test or panic.
extern "x86-interrupt" fn page_fault_handler(
    mut stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    let cr2 = Cr2::read_raw();
    let from_user = (stack_frame.code_segment.0 & 3) == 3;

    if from_user {
        serial_println!(
            "USER #PF: cr2={:#x} err={:?} rip={:#x}",
            cr2,
            error_code,
            stack_frame.instruction_pointer.as_u64()
        );
        // M11a: just halt the CPU. M12 will signal the process and reschedule.
        loop {
            x86_64::instructions::hlt();
        }
    }

    let fixup = EXPECTED_FAULT_FIXUP.swap(0, Ordering::SeqCst);
    if fixup != 0 {
        // SAFETY: rewriting the saved RIP so `iretq` resumes at the fixup
        // label. Single-CPU; no other handler is racing on this stack frame.
        unsafe {
            let mut volatile = stack_frame.as_mut();
            let mut value = volatile.read();
            value.instruction_pointer = x86_64::VirtAddr::new(fixup as u64);
            volatile.write(value);
        }
        return;
    }

    serial_println!(
        "KERNEL #PF: cr2={:#x} err={:?} rip={:#x}",
        cr2,
        error_code,
        stack_frame.instruction_pointer.as_u64()
    );
    exit_qemu(QemuExitCode::Failed);
}

extern "x86-interrupt" fn gp_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    serial_println!(
        "EXCEPTION: #GP err={:#x} cs={:#x} rip={:#x}",
        error_code,
        stack_frame.code_segment.0,
        stack_frame.instruction_pointer.as_u64()
    );
    exit_qemu(QemuExitCode::Failed);
}

extern "x86-interrupt" fn ud_handler(stack_frame: InterruptStackFrame) {
    serial_println!(
        "EXCEPTION: #UD cs={:#x} rip={:#x}",
        stack_frame.code_segment.0,
        stack_frame.instruction_pointer.as_u64()
    );
    exit_qemu(QemuExitCode::Failed);
}

extern "x86-interrupt" fn timer_handler(_stack_frame: InterruptStackFrame) {
    TIMER_TICKS.fetch_add(1, Ordering::Relaxed);
    unsafe { PICS.lock().notify_end_of_interrupt(Irq::Timer.as_u8()) };
}

extern "x86-interrupt" fn keyboard_handler(_stack_frame: InterruptStackFrame) {
    use x86_64::instructions::port::Port;
    let mut port: Port<u8> = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };
    crate::task::keyboard::add_scancode(scancode);
    unsafe { PICS.lock().notify_end_of_interrupt(Irq::Keyboard.as_u8()) };
}
