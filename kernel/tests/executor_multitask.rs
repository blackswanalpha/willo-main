#![no_std]
#![no_main]

extern crate alloc;

use bootloader_api::{BootInfo, entry_point};
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicU32, Ordering};
use core::task::{Context, Poll};
use kernel::task::{Task, executor::Executor};
use kernel::{QemuExitCode, exit_qemu, serial_println};
use x86_64::VirtAddr;

entry_point!(test_main, config = &kernel::BOOTLOADER_CONFIG);

static A: AtomicU32 = AtomicU32::new(0);
static B: AtomicU32 = AtomicU32::new(0);

fn test_main(boot_info: &'static mut BootInfo) -> ! {
    kernel::serial::init();
    kernel::gdt::init();
    kernel::interrupts::init();

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

    serial_println!("executor_multitask::round_robin... [running]");

    let mut executor = Executor::new();
    executor.spawn(Task::new(task_a()));
    executor.spawn(Task::new(task_b()));
    executor.run();
}

async fn task_a() {
    for _ in 0..5 {
        A.fetch_add(1, Ordering::Relaxed);
        Yield::default().await;
    }
}

async fn task_b() {
    for _ in 0..5 {
        B.fetch_add(1, Ordering::Relaxed);
        Yield::default().await;
    }
    let a = A.load(Ordering::Relaxed);
    let b = B.load(Ordering::Relaxed);
    if a == 5 && b == 5 {
        serial_println!("[ok] both tasks reached 5 (round-robin works)");
        exit_qemu(QemuExitCode::Success);
    } else {
        serial_println!("[FAILED] A={a} B={b}");
        exit_qemu(QemuExitCode::Failed);
    }
}

#[derive(Default)]
struct Yield {
    yielded: bool,
}

impl Future for Yield {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<()> {
        if self.yielded {
            Poll::Ready(())
        } else {
            self.yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    serial_println!("[FAILED] panic: {info}");
    exit_qemu(QemuExitCode::Failed);
}
