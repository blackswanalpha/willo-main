#![no_std]
#![no_main]

use bootloader_api::{BootInfo, entry_point};
use kernel::{QemuExitCode, exit_qemu, serial_println};

entry_point!(test_main);

fn test_main(_boot_info: &'static mut BootInfo) -> ! {
    kernel::serial::init();
    serial_println!("breakpoint::int3_returns... [running]");

    kernel::gdt::init();
    kernel::interrupts::init();

    x86_64::instructions::interrupts::int3();

    serial_println!("[ok] int3 returned cleanly");
    exit_qemu(QemuExitCode::Success);
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    serial_println!("[FAILED] panic: {info}");
    exit_qemu(QemuExitCode::Failed);
}
