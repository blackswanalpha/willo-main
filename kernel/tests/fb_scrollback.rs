#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use bootloader_api::info::{FrameBufferInfo, PixelFormat};
use bootloader_api::{BootInfo, entry_point};
use core::fmt::Write;
use kernel::framebuffer::{FrameBufferWriter, SCROLLBACK_LINES};
use kernel::{QemuExitCode, exit_qemu, serial_println};
use x86_64::VirtAddr;

entry_point!(test_main, config = &kernel::BOOTLOADER_CONFIG);

fn test_main(boot_info: &'static mut BootInfo) -> ! {
    kernel::serial::init();
    kernel::gdt::init();
    kernel::interrupts::init();
    serial_println!("fb_scrollback::ring_and_scroll... [running]");

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

    // Tiny heap-backed framebuffer just for this test. The kernel heap is
    // 256 KiB so we keep the backing well under that — we only care about
    // ring/scrollback semantics, not pixel-perfect rendering. Resolution
    // chosen so viewport_rows ≥ 5 with the size-16 font's 18-pixel rows.
    let width = 240usize;
    let height = 160usize;
    let bpp = 4usize;
    let stride = width;
    let info = FrameBufferInfo {
        byte_len: stride * height * bpp,
        width,
        height,
        pixel_format: PixelFormat::Rgb,
        bytes_per_pixel: bpp,
        stride,
    };
    let mut backing = alloc::vec![0u8; info.byte_len];
    let buf: &'static mut [u8] = unsafe {
        core::slice::from_raw_parts_mut(backing.as_mut_ptr(), backing.len())
    };
    let mut w = FrameBufferWriter::new(buf, info);

    // Print 1024 distinct numbered lines. Capacity is SCROLLBACK_LINES (256),
    // so the ring will have wrapped to retain only the most recent 256 lines.
    for i in 0..1024usize {
        writeln!(w, "line {i}").expect("write");
    }

    let snap = w.ring_snapshot();
    assert_eq!(snap.len(), SCROLLBACK_LINES, "ring size: {}", snap.len());
    // Newest line in the ring should be 1023.
    assert_eq!(snap.last().unwrap(), &format!("line 1023"));
    // Oldest retained line should be 1024 - 256 = 768.
    assert_eq!(snap.first().unwrap(), &format!("line 768"));

    // Scroll back 100 lines; the writer must clamp to a valid offset and
    // never panic on the redraw. (Actual blits go to the heap buffer.)
    let viewport = w.viewport_rows();
    w.scroll_up(100);
    let new_back = w.scroll_back();
    assert!(new_back > 0, "scroll_up did not advance");
    assert!(
        new_back <= snap.len() + 1 - viewport,
        "scroll_back {new_back} exceeded max for viewport {viewport}"
    );

    // End snaps to the live tail.
    w.scroll_end();
    assert_eq!(w.scroll_back(), 0);

    // Keep `backing` alive for the &'static slice we leaked into the writer.
    core::mem::forget(backing);

    serial_println!("[ok] fb scrollback ring + scroll_up/end pass");
    exit_qemu(QemuExitCode::Success);
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    serial_println!("[FAILED] panic: {info}");
    exit_qemu(QemuExitCode::Failed);
}
