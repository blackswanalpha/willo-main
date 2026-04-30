#![no_std]
#![feature(abi_x86_interrupt)]

extern crate alloc;

pub mod allocator;
pub mod ata;
pub mod block;
pub mod framebuffer;
pub mod fs;
pub mod gdt;
pub mod interrupts;
pub mod memory;
pub mod serial;
pub mod shell;
pub mod task;

use bootloader_api::config::{BootloaderConfig, Mapping};
use bootloader_api::info::FrameBuffer;
use conquer_once::spin::OnceCell;
use core::fmt;
use framebuffer::FrameBufferWriter;
use spin::Mutex;

/// Bootloader config used by every kernel/test entry point.
///
/// Asks the bootloader to identity-map all physical memory at a dynamically
/// chosen virtual offset, which `memory::init` then turns into an
/// `OffsetPageTable` we can hand to `Mapper`-using code.
pub static BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();
    config.mappings.physical_memory = Some(Mapping::Dynamic);
    config
};

pub static FB_WRITER: OnceCell<Mutex<FrameBufferWriter>> = OnceCell::uninit();

pub fn init_framebuffer(fb: &'static mut FrameBuffer) {
    let info = fb.info();
    let raw = fb.buffer_mut();
    let ptr = raw.as_mut_ptr();
    let len = raw.len();
    let buf: &'static mut [u8] = unsafe { core::slice::from_raw_parts_mut(ptr, len) };
    FB_WRITER.init_once(|| Mutex::new(FrameBufferWriter::new(buf, info)));
}

pub fn init() {
    serial::init();
    gdt::init();
    interrupts::init();
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    use x86_64::instructions::interrupts;
    interrupts::without_interrupts(|| {
        if let Some(w) = FB_WRITER.get() {
            let _ = w.lock().write_fmt(args);
        }
    });
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum QemuExitCode {
    Success = 0x10,
    Failed = 0x11,
}

pub fn exit_qemu(code: QemuExitCode) -> ! {
    use x86_64::instructions::{nop, port::Port};
    unsafe {
        Port::new(0xf4).write(code as u32);
    }
    loop {
        nop();
    }
}

pub fn hlt_loop() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}
