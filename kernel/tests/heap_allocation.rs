#![no_std]
#![no_main]

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use bootloader_api::{BootInfo, entry_point};
use kernel::{QemuExitCode, exit_qemu, serial_println};
use x86_64::VirtAddr;

entry_point!(test_main, config = &kernel::BOOTLOADER_CONFIG);

fn test_main(boot_info: &'static mut BootInfo) -> ! {
    kernel::serial::init();
    kernel::gdt::init();
    kernel::interrupts::init();
    serial_println!("heap_allocation::box_vec_string_burst... [running]");

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

    // Single Box round-trips
    let a = Box::new(41);
    let b = Box::new(13);
    assert_eq!(*a, 41);
    assert_eq!(*b, 13);

    // Larger Vec: read-back + sum
    let n: u64 = 1000;
    let mut v: Vec<u64> = Vec::with_capacity(n as usize);
    for i in 0..n {
        v.push(i);
    }
    assert_eq!(v.iter().sum::<u64>(), (n - 1) * n / 2);

    // Burst of short-lived allocations to force the linked-list allocator to
    // reuse freed blocks instead of growing forever.
    for i in 0..(kernel::allocator::HEAP_SIZE * 2) {
        let x = Box::new(i);
        assert_eq!(*x, i);
    }

    // String works too
    let mut s = String::new();
    s.push_str("hello, kernel heap");
    assert_eq!(s.len(), 18);

    serial_println!(
        "[ok] Box, Vec({n}), {} short-lived allocs, String — all good",
        kernel::allocator::HEAP_SIZE * 2
    );
    exit_qemu(QemuExitCode::Success);
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    serial_println!("[FAILED] panic: {info}");
    exit_qemu(QemuExitCode::Failed);
}
