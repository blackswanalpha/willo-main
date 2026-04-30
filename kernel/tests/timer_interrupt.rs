#![no_std]
#![no_main]

use bootloader_api::{BootInfo, entry_point};
use core::sync::atomic::Ordering;
use kernel::{QemuExitCode, exit_qemu, interrupts::TIMER_TICKS, serial_println};

entry_point!(test_main);

fn test_main(_boot_info: &'static mut BootInfo) -> ! {
    kernel::init();
    serial_println!("timer_interrupt::ticks_advance... [running]");

    // Spin (with `hlt` so we yield to the timer IRQ) until the counter advances.
    // If the IRQ pipeline (PIC remap + IDT vector + sti) is broken, this blocks
    // forever and the test runner times out — also a fail.
    let target: u64 = 5;
    loop {
        if TIMER_TICKS.load(Ordering::Relaxed) >= target {
            break;
        }
        x86_64::instructions::hlt();
    }

    serial_println!(
        "[ok] observed {} timer ticks via PIC IRQ0",
        TIMER_TICKS.load(Ordering::Relaxed)
    );
    exit_qemu(QemuExitCode::Success);
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    serial_println!("[FAILED] panic: {info}");
    exit_qemu(QemuExitCode::Failed);
}
