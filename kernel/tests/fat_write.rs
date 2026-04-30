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
    serial_println!("fat_write::create_write_delete... [running]");

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

    // 1. Create a fresh file at root and read it back.
    fs.write_file("/notes.txt", b"first line\n").expect("write notes");
    let read = fs.read_file("/notes.txt").expect("read notes");
    assert_eq!(&read, b"first line\n");

    // 1b. Append a second line — exercises the M10 read-extend-rewrite path.
    fs.append_file("/notes.txt", b"second line\n").expect("append notes");
    let read = fs.read_file("/notes.txt").expect("read appended notes");
    assert_eq!(&read, b"first line\nsecond line\n", "append mismatch");

    // 1c. Append to a path that does not exist — should create.
    fs.append_file("/created.txt", b"hello\n").expect("append-create");
    let read = fs.read_file("/created.txt").expect("read created");
    assert_eq!(&read, b"hello\n");
    fs.remove("/created.txt").expect("rm created");

    // 2. Overwrite with longer content (forces FAT chain reuse + extension).
    let big = make_filler(2500);
    fs.write_file("/notes.txt", &big).expect("rewrite notes");
    let read = fs.read_file("/notes.txt").expect("re-read notes");
    assert_eq!(read, big, "rewrite mismatch");

    // 3. Make a new directory and put a file inside it.
    fs.create_dir("/work").expect("mkdir work");
    assert!(fs.is_dir("/work"));
    fs.write_file("/work/log.txt", b"hello from /work\n").expect("write log");
    let log = fs.read_file("/work/log.txt").expect("read log");
    assert_eq!(&log, b"hello from /work\n");

    // 4. Listing reflects the new entries.
    let mut root = fs.list_dir("/").expect("list /");
    root.sort();
    assert!(root.iter().any(|n| n == "notes.txt"), "got {root:?}");
    assert!(root.iter().any(|n| n == "work"), "got {root:?}");

    // 5. Remove the file inside /work, then the empty dir, then /notes.txt.
    fs.remove("/work/log.txt").expect("rm log");
    assert!(matches!(
        fs.read_file("/work/log.txt"),
        Err(kernel::fs::FsError::NotFound)
    ));
    fs.remove("/work").expect("rmdir work");
    assert!(!fs.is_dir("/work"));
    fs.remove("/notes.txt").expect("rm notes");

    let mut root = fs.list_dir("/").expect("re-list /");
    root.sort();
    assert!(!root.iter().any(|n| n == "notes.txt"), "still there: {root:?}");
    assert!(!root.iter().any(|n| n == "work"), "still there: {root:?}");

    serial_println!("[ok] FAT32 write/create/dir/remove all pass");
    exit_qemu(QemuExitCode::Success);
}

fn make_filler(len: usize) -> alloc::vec::Vec<u8> {
    let mut v = alloc::vec::Vec::with_capacity(len);
    for i in 0..len {
        v.push(((i % 26) as u8) + b'a');
    }
    v
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    serial_println!("[FAILED] panic: {info}");
    exit_qemu(QemuExitCode::Failed);
}
