use conquer_once::spin::OnceCell;
use core::fmt;
use spin::Mutex;
use uart_16550::SerialPort;

pub static SERIAL1: OnceCell<Mutex<SerialPort>> = OnceCell::uninit();

pub fn init() {
    SERIAL1.init_once(|| {
        let mut port = unsafe { SerialPort::new(0x3F8) };
        port.init();
        Mutex::new(port)
    });
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    use x86_64::instructions::interrupts;
    interrupts::without_interrupts(|| {
        if let Some(p) = SERIAL1.get() {
            let _ = p.lock().write_fmt(args);
        }
    });
}

#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => ($crate::serial::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial_print!("\n"));
    ($($arg:tt)*) => ($crate::serial_print!("{}\n", format_args!($($arg)*)));
}
