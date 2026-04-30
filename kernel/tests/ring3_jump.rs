//! M11a smoke test: drop to ring 3 once and confirm it.
//!
//! Maps a single user-half page with `USER|EXECUTE`, copies an `int 3` stub
//! into it, builds an `iretq` frame with user CS/SS (RPL=3), then `iretq`s
//! into the stub. The first instruction (`0xCC`) traps to the kernel's
//! breakpoint handler, which calls our `BP_HOOK` — that's where we assert
//! `(CS & 3) == 3 && (SS & 3) == 3` and exit QEMU with success.
//!
//! M11a deliberately uses the *active* PML4: there's no `AddrSpace`
//! abstraction yet, so we flip a single PTE to `USER` in the boot-time page
//! table. M11c will replace this with per-process page tables.

#![no_std]
#![no_main]

use bootloader_api::{BootInfo, entry_point};
use core::arch::asm;
use kernel::{QemuExitCode, allocator, exit_qemu, memory, serial_println};
use x86_64::VirtAddr;
use x86_64::structures::idt::InterruptStackFrame;
use x86_64::structures::paging::{Mapper, Page, PageTableFlags, Size4KiB};

entry_point!(test_main, config = &kernel::BOOTLOADER_CONFIG);

const USER_CODE_VADDR: u64 = 0x4000_0000;
const USER_STACK_VADDR: u64 = 0x5000_0000;

fn test_main(boot_info: &'static mut BootInfo) -> ! {
    kernel::serial::init();
    kernel::gdt::init();
    kernel::interrupts::BP_HOOK.call_once(|| ring3_bp_hook);
    kernel::interrupts::init();

    let phys_offset = VirtAddr::new(
        boot_info
            .physical_memory_offset
            .into_option()
            .expect("phys offset"),
    );
    let mut mapper = unsafe { memory::init(phys_offset) };
    let mut frame_allocator =
        unsafe { memory::BootInfoFrameAllocator::new(&boot_info.memory_regions) };
    allocator::init_heap(&mut mapper, &mut frame_allocator).expect("heap");

    serial_println!("[ring3_jump] mapping user code + stack");
    map_user_page(
        &mut mapper,
        &mut frame_allocator,
        USER_CODE_VADDR,
        // M11a: writable+executable so the kernel can stage the stub byte.
        // M11c will introduce W^X via the AddrSpace abstraction.
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE,
    );
    map_user_page(
        &mut mapper,
        &mut frame_allocator,
        USER_STACK_VADDR,
        PageTableFlags::PRESENT
            | PageTableFlags::WRITABLE
            | PageTableFlags::USER_ACCESSIBLE
            | PageTableFlags::NO_EXECUTE,
    );

    // `int 3` (0xCC) — single-byte software-breakpoint instruction. INT3 from
    // ring 3 does NOT do the IDT DPL check (per Intel SDM); kernel's
    // breakpoint handler will fire and we capture the saved CS/SS.
    unsafe {
        let code_ptr = USER_CODE_VADDR as *mut u8;
        code_ptr.write_volatile(0xCC);
    }

    let sels = kernel::gdt::selectors();
    let user_cs = sels.user_code_selector.0 | 3;
    let user_ss = sels.user_data_selector.0 | 3;
    let user_rip = USER_CODE_VADDR;
    let user_rsp = USER_STACK_VADDR + 0x1000 - 16;

    serial_println!(
        "[ring3_jump] iretq cs={:#x} ss={:#x} rip={:#x} rsp={:#x}",
        user_cs,
        user_ss,
        user_rip,
        user_rsp
    );

    // Build the iretq frame and dive. On return from int3, control flows back
    // through `breakpoint_handler` -> `ring3_bp_hook` -> `exit_qemu`. The
    // `iretq` sequence will not return here.
    unsafe {
        asm!(
            "push {ss}",
            "push {rsp}",
            "push 0x202",        // RFLAGS: IF=1, reserved bit 1 set
            "push {cs}",
            "push {rip}",
            "iretq",
            ss = in(reg) user_ss as u64,
            rsp = in(reg) user_rsp,
            cs = in(reg) user_cs as u64,
            rip = in(reg) user_rip,
            options(noreturn),
        );
    }
}

fn map_user_page(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut kernel::memory::BootInfoFrameAllocator,
    vaddr: u64,
    flags: PageTableFlags,
) {
    use x86_64::structures::paging::FrameAllocator;
    let page: Page<Size4KiB> = Page::containing_address(VirtAddr::new(vaddr));
    let frame = frame_allocator.allocate_frame().expect("frame");
    unsafe {
        mapper
            .map_to(page, frame, flags, frame_allocator)
            .expect("map_to")
            .flush();
    }
}

fn ring3_bp_hook(frame: &InterruptStackFrame) -> ! {
    let cs = frame.code_segment.0;
    let ss = frame.stack_segment.0;
    serial_println!("[ring3_jump] BP captured cs={:#x} ss={:#x}", cs, ss);
    if (cs & 3) == 3 && (ss & 3) == 3 {
        serial_println!("[ring3_jump] PASS — ring 3 confirmed");
        exit_qemu(QemuExitCode::Success);
    }
    serial_println!("[ring3_jump] FAIL — expected ring 3 but cs/ss not RPL3");
    exit_qemu(QemuExitCode::Failed);
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    serial_println!("[ring3_jump] PANIC: {info}");
    exit_qemu(QemuExitCode::Failed);
}
