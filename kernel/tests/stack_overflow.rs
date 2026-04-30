#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

use bootloader_api::{BootInfo, entry_point};
use kernel::{QemuExitCode, exit_qemu, serial_println};
use spin::Lazy;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};

entry_point!(test_main);

fn test_main(_boot_info: &'static mut BootInfo) -> ! {
    kernel::serial::init();
    serial_println!("stack_overflow::stack_overflow... [running]");

    kernel::gdt::init();
    TEST_IDT.load();

    overflow();

    serial_println!("[FAILED] expected double fault, kernel kept running");
    exit_qemu(QemuExitCode::Failed);
}

#[allow(unconditional_recursion)]
fn overflow() {
    overflow();
    // prevent tail-call optimisation by making the compiler think this side-effects
    core::hint::black_box(0u8);
}

static TEST_IDT: Lazy<InterruptDescriptorTable> = Lazy::new(|| {
    let mut idt = InterruptDescriptorTable::new();
    unsafe {
        idt.double_fault
            .set_handler_fn(test_double_fault)
            .set_stack_index(kernel::gdt::DOUBLE_FAULT_IST_INDEX);
    }
    idt
});

extern "x86-interrupt" fn test_double_fault(
    _stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    serial_println!("[ok] double fault captured (kernel-stack overflow contained)");
    exit_qemu(QemuExitCode::Success);
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    serial_println!("[FAILED] panic: {info}");
    exit_qemu(QemuExitCode::Failed);
}
