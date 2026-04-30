#![no_std]
#![no_main]

extern crate alloc;

use bootloader_api::{BootInfo, entry_point};
use kernel::fs::{FileSystem, FsError, tmp::TmpFs};
use kernel::{QemuExitCode, exit_qemu, serial_println};
use x86_64::VirtAddr;

entry_point!(test_main, config = &kernel::BOOTLOADER_CONFIG);

fn test_main(boot_info: &'static mut BootInfo) -> ! {
    kernel::serial::init();
    kernel::gdt::init();
    kernel::interrupts::init();
    serial_println!("tmpfs::write_read_clear... [running]");

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

    let tmp = TmpFs::new();

    // 1. write/read across two paths.
    tmp.create_dir("/work").expect("mkdir /work");
    tmp.write_file("/work/log", b"line1\n").expect("write log");
    tmp.write_file("/note", b"jot\n").expect("write note");

    assert_eq!(tmp.read_file("/work/log").unwrap(), b"line1\n".to_vec());
    assert_eq!(tmp.read_file("/note").unwrap(), b"jot\n".to_vec());

    // 2. listing reflects entries.
    let mut root = tmp.list_dir("/").unwrap();
    root.sort();
    assert!(root.iter().any(|n| n == "work"), "got {root:?}");
    assert!(root.iter().any(|n| n == "note"), "got {root:?}");

    // 3. append extends the file.
    tmp.append_file("/work/log", b"line2\n").expect("append log");
    assert_eq!(
        tmp.read_file("/work/log").unwrap(),
        b"line1\nline2\n".to_vec()
    );

    // 4. removal works; non-empty dir refuses removal.
    assert!(matches!(tmp.remove("/work"), Err(FsError::Io)));
    tmp.remove("/work/log").expect("rm log");
    tmp.remove("/work").expect("rm /work");
    assert!(matches!(tmp.read_file("/work/log"), Err(FsError::NotFound)));

    // 5. cleared on reboot — a fresh TmpFs has nothing in it. This is the
    //    same path mount_root() takes on every kernel boot.
    let fresh = TmpFs::new();
    let listing = fresh.list_dir("/").unwrap();
    assert!(listing.is_empty(), "fresh tmpfs not empty: {listing:?}");

    serial_println!("[ok] tmpfs write/read/append/clear all pass");
    exit_qemu(QemuExitCode::Success);
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    serial_println!("[FAILED] panic: {info}");
    exit_qemu(QemuExitCode::Failed);
}
