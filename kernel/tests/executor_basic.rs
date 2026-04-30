#![no_std]
#![no_main]

extern crate alloc;

use bootloader_api::{BootInfo, entry_point};
use kernel::task::{Task, executor::Executor};
use kernel::{QemuExitCode, exit_qemu, serial_println};
use x86_64::VirtAddr;

entry_point!(test_main, config = &kernel::BOOTLOADER_CONFIG);

fn test_main(boot_info: &'static mut BootInfo) -> ! {
    kernel::serial::init();
    kernel::gdt::init();
    kernel::interrupts::init();

    let phys_offset = VirtAddr::new(
        boot_info
            .physical_memory_offset
            .into_option()
            .expect("bootloader did not map physical memory"),
    );
    let mut mapper = unsafe { kernel::memory::init(phys_offset) };
    let mut frame_allocator =
        unsafe { kernel::memory::BootInfoFrameAllocator::new(&boot_info.memory_regions) };
    kernel::allocator::init_heap(&mut mapper, &mut frame_allocator).expect("heap init");

    serial_println!("executor_basic::async_chain_runs... [running]");

    let mut executor = Executor::new();
    executor.spawn(Task::new(driver()));
    executor.run();
}

async fn driver() {
    let v = produce().await;
    if v == 42 {
        serial_println!("[ok] async task produced {v}");
        exit_qemu(QemuExitCode::Success);
    } else {
        serial_println!("[FAILED] expected 42, got {v}");
        exit_qemu(QemuExitCode::Failed);
    }
}

async fn produce() -> u32 {
    42
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    serial_println!("[FAILED] panic: {info}");
    exit_qemu(QemuExitCode::Failed);
}
