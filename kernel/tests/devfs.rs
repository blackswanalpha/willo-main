#![no_std]
#![no_main]

extern crate alloc;

use bootloader_api::{BootInfo, entry_point};
use kernel::fs::{FileSystem, dev::DevFs};
use kernel::{QemuExitCode, exit_qemu, serial_println};
use x86_64::VirtAddr;

entry_point!(test_main, config = &kernel::BOOTLOADER_CONFIG);

fn test_main(boot_info: &'static mut BootInfo) -> ! {
    kernel::serial::init();
    kernel::gdt::init();
    kernel::interrupts::init();
    serial_println!("devfs::null_zero_serial... [running]");

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

    let dev = DevFs::new();

    // list_dir at root surfaces the static node names.
    let mut entries = dev.list_dir("/").unwrap();
    entries.sort();
    let names = ["console", "disk0", "null", "serial0", "zero"];
    for n in &names {
        assert!(entries.iter().any(|e| e == n), "missing {n}: {entries:?}");
    }

    // Read 4 KiB of /dev/zero via read_at; expect all zero.
    let mut buf = alloc::vec![0xFFu8; 4096];
    let n = dev.read_at("/zero", 0, &mut buf).expect("read zero");
    assert_eq!(n, 4096);
    assert!(buf.iter().all(|&b| b == 0), "/dev/zero not all-zero");

    // /dev/null discards writes.
    let n = dev.write_at("/null", 0, b"discard me").expect("write null");
    assert_eq!(n, b"discard me".len());

    // /dev/null reads return EOF immediately.
    let mut sink = [0u8; 8];
    let n = dev.read_at("/null", 0, &mut sink).expect("read null");
    assert_eq!(n, 0);

    // /dev/serial0 writes pass through to the UART (visible in test output).
    let msg = b"[devfs] hello from /dev/serial0\n";
    let n = dev.write_at("/serial0", 0, msg).expect("write serial0");
    assert_eq!(n, msg.len());

    // read_file convenience: zero gives a fixed block, null is empty.
    let z = dev.read_file("/zero").unwrap();
    assert_eq!(z.len(), 4096);
    assert!(z.iter().all(|&b| b == 0));
    let null_read = dev.read_file("/null").unwrap();
    assert!(null_read.is_empty());

    // is_dir semantics.
    assert!(dev.is_dir("/"));
    assert!(!dev.is_dir("/zero"));

    serial_println!("[ok] devfs list/null/zero/serial0 all pass");
    exit_qemu(QemuExitCode::Success);
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    serial_println!("[FAILED] panic: {info}");
    exit_qemu(QemuExitCode::Failed);
}
