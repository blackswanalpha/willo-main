#![no_std]
#![no_main]

extern crate alloc;

use bootloader_api::{BootInfo, entry_point};
use kernel::fs::{self, FsError, ram};
use kernel::{QemuExitCode, exit_qemu, serial_println};
use x86_64::VirtAddr;

entry_point!(test_main, config = &kernel::BOOTLOADER_CONFIG);

fn test_main(boot_info: &'static mut BootInfo) -> ! {
    kernel::serial::init();
    kernel::gdt::init();
    kernel::interrupts::init();
    serial_println!("ramfs::lookup_read_list... [running]");

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

    // Test the in-memory backend directly so this test stays deterministic
    // regardless of whether a FAT data disk is attached this run.
    let r = ram::seed();

    assert_eq!(
        r.read_file("/etc/version").unwrap().as_slice(),
        b"willo 0.9 (M9)\n"
    );
    assert!(matches!(r.read_file("/etc"), Err(FsError::IsADir)));
    assert!(matches!(r.read_file("/nope"), Err(FsError::NotFound)));

    let mut etc_entries = r.list_dir("/etc").unwrap();
    etc_entries.sort();
    assert_eq!(etc_entries, ["motd", "version"]);

    assert!(r.is_dir("/"));
    assert!(r.is_dir("/proc"));
    assert!(!r.is_dir("/welcome.txt"));

    assert_eq!(fs::normalize("/etc", "../proc/./uptime"), "/proc/uptime");
    assert_eq!(fs::normalize("/", "../.."), "/");
    assert_eq!(fs::normalize("/etc", "/bin"), "/bin");
    assert_eq!(fs::normalize("/", "."), "/");

    let uptime = r.read_file("/proc/uptime").unwrap();
    let s = core::str::from_utf8(&uptime).expect("uptime is utf8");
    assert!(s.ends_with(" ticks\n"), "got {s:?}");

    serial_println!("[ok] ramfs lookup/read/list/normalize all pass");
    exit_qemu(QemuExitCode::Success);
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    serial_println!("[FAILED] panic: {info}");
    exit_qemu(QemuExitCode::Failed);
}
