#![no_std]
#![no_main]

use bootloader_api::{BootInfo, entry_point};
use kernel::{QemuExitCode, exit_qemu, serial_println};

entry_point!(test_main);

fn test_main(_boot_info: &'static mut BootInfo) -> ! {
    kernel::serial::init();
    serial_println!("should_panic::deliberate_assertion_failure... [running]");
    deliberate_assertion_failure();
    // If we ever reach here, the panic handler did not fire — that's a failure.
    serial_println!("[FAILED] panic was expected but never fired");
    exit_qemu(QemuExitCode::Failed);
}

fn deliberate_assertion_failure() {
    assert_eq!(0, 1, "this assertion is expected to fail");
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    serial_println!("[ok] panic captured as expected");
    exit_qemu(QemuExitCode::Success);
}
