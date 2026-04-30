#![no_std]
#![no_main]

extern crate alloc;

use bootloader_api::{BootInfo, entry_point};
use kernel::ata;
use kernel::fs::fat::FatFs;
use kernel::{QemuExitCode, exit_qemu, serial_println};
use x86_64::VirtAddr;

entry_point!(test_main, config = &kernel::BOOTLOADER_CONFIG);

fn test_main(boot_info: &'static mut BootInfo) -> ! {
    kernel::serial::init();
    kernel::gdt::init();
    kernel::interrupts::init();
    serial_println!("fat_read::mount_and_lookup... [running]");

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

    ata::init().expect("data disk attached");
    let fs = FatFs::mount().expect("FAT mount");

    serial_println!(
        "  bps={} spc={} root_cluster={} fat_lba={} data_lba={}",
        fs.bytes_per_sector,
        fs.sectors_per_cluster,
        fs.root_cluster,
        fs.fat_start_lba,
        fs.data_start_lba,
    );

    assert!(fs.is_dir("/"), "root must be a directory");
    assert!(fs.is_dir("/etc"), "/etc must exist as directory");
    assert!(!fs.is_dir("/welcome.txt"), "welcome.txt is a file");

    let welcome = fs.read_file("/welcome.txt").expect("read welcome");
    let s = core::str::from_utf8(&welcome).expect("utf8");
    assert!(s.contains("Welcome to Willo"), "got {s:?}");

    let version = fs.read_file("/etc/version").expect("read version");
    let v = core::str::from_utf8(&version).expect("utf8");
    assert!(v.starts_with("willo 0.9"), "got {v:?}");

    let mut entries = fs.list_dir("/").expect("list root");
    entries.sort();
    // The runner seeds: bin (dir), etc (dir), welcome.txt (file).
    assert!(entries.iter().any(|n| n == "etc"), "got {entries:?}");
    assert!(entries.iter().any(|n| n == "welcome.txt"), "got {entries:?}");

    let mut etc_entries = fs.list_dir("/etc").expect("list etc");
    etc_entries.sort();
    assert_eq!(etc_entries, ["motd", "version"]);

    serial_println!("[ok] FAT32 mount + lookup + read all pass");
    exit_qemu(QemuExitCode::Success);
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    serial_println!("[FAILED] panic: {info}");
    exit_qemu(QemuExitCode::Failed);
}
