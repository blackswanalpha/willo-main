#![no_std]
#![no_main]

extern crate alloc;

use bootloader_api::{BootInfo, entry_point};
use kernel::shell::shell_loop;
use kernel::task::{Task, executor::Executor};
use kernel::{
    QemuExitCode, allocator, exit_qemu, init_framebuffer, memory, println, serial_println,
};
use x86_64::VirtAddr;

entry_point!(kernel_main, config = &kernel::BOOTLOADER_CONFIG);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    kernel::init();
    serial_println!("");
    serial_println!("============================================");
    serial_println!("  Willo kernel — M10 multi-mount + scrollback");
    serial_println!("============================================");

    let phys_offset = VirtAddr::new(
        boot_info
            .physical_memory_offset
            .into_option()
            .expect("bootloader did not map physical memory"),
    );
    let mut mapper = unsafe { memory::init(phys_offset) };
    let mut frame_allocator =
        unsafe { memory::BootInfoFrameAllocator::new(&boot_info.memory_regions) };
    allocator::init_heap(&mut mapper, &mut frame_allocator).expect("heap init failed");
    serial_println!(
        "[stage] heap up @ {:#x} ({} KiB)",
        allocator::HEAP_START,
        allocator::HEAP_SIZE / 1024
    );

    if let Some(fb) = boot_info.framebuffer.as_mut() {
        init_framebuffer(fb);
        println!("Willo M10 — type `help`");
    }

    let vfs = kernel::fs::mount_root();

    let mut executor = Executor::new();
    executor.spawn(Task::new(shell_loop(vfs)));
    executor.run();
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    serial_println!("PANIC: {info}");
    exit_qemu(QemuExitCode::Failed);
}
