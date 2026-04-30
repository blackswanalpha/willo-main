#![no_std]
#![no_main]

extern crate alloc;

use bootloader_api::{BootInfo, entry_point};
use kernel::ata;
use kernel::block::BlockDevice;
use kernel::{QemuExitCode, exit_qemu, serial_println};
use x86_64::VirtAddr;

entry_point!(test_main, config = &kernel::BOOTLOADER_CONFIG);

fn test_main(boot_info: &'static mut BootInfo) -> ! {
    kernel::serial::init();
    kernel::gdt::init();
    kernel::interrupts::init();
    serial_println!("ata_identify::primary_slave... [running]");

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

    ata::init().expect("data disk should be attached as primary IDE slave");

    let n = ata::DRIVE.lock().num_blocks();
    assert!(n >= 1024, "data disk too small: {n} sectors");

    // Read sector 0 (the FAT BPB) and sanity-check the boot signature.
    let mut buf = [0u8; 512];
    ata::DRIVE
        .lock()
        .read_block(0, &mut buf)
        .expect("read sector 0");
    assert_eq!(buf[510], 0x55);
    assert_eq!(buf[511], 0xAA);

    serial_println!("[ok] ATA primary slave identified, {n} sectors, boot sig OK");
    exit_qemu(QemuExitCode::Success);
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    serial_println!("[FAILED] panic: {info}");
    exit_qemu(QemuExitCode::Failed);
}
