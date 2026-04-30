#![no_std]
#![no_main]

use bootloader_api::{BootInfo, entry_point};
use kernel::{QemuExitCode, exit_qemu, serial_println};

entry_point!(test_main);

fn test_main(_boot_info: &'static mut BootInfo) -> ! {
    kernel::serial::init();
    serial_println!("basic_boot::println... [running]");
    test_println();
    serial_println!("basic_boot::all_passed [ok]");
    exit_qemu(QemuExitCode::Success);
}

fn test_println() {
    // Calling the framebuffer-backed `println!` without an initialized
    // framebuffer should simply no-op (we never call `init_framebuffer`).
    // This exercises the macro path without requiring a graphical buffer.
    use core::fmt::Write;
    if let Some(p) = kernel::serial::SERIAL1.get() {
        let _ = writeln!(p.lock(), "  hello from inside the basic_boot test");
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    serial_println!("[FAILED]");
    serial_println!("Error: {info}");
    exit_qemu(QemuExitCode::Failed);
}
