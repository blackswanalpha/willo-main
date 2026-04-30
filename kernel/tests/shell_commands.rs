#![no_std]
#![no_main]

extern crate alloc;

use bootloader_api::{BootInfo, entry_point};
use kernel::fs::mount_root;
use kernel::shell::Shell;
use kernel::{QemuExitCode, exit_qemu, serial_println};
use x86_64::VirtAddr;

entry_point!(test_main, config = &kernel::BOOTLOADER_CONFIG);

fn test_main(boot_info: &'static mut BootInfo) -> ! {
    kernel::serial::init();
    kernel::gdt::init();
    kernel::interrupts::init();
    serial_println!("shell_commands::execute_line... [running]");

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

    let mut sh = Shell::new(mount_root());

    assert_eq!(sh.execute_line("pwd"), "/\n");
    assert_eq!(sh.execute_line(""), "");

    let ls_root = sh.execute_line("ls");
    assert!(ls_root.contains("etc/"), "got {ls_root:?}");
    assert!(ls_root.contains("welcome.txt"), "got {ls_root:?}");
    // Mount points surface as virtual dir children of `/`.
    assert!(ls_root.contains("tmp/"), "got {ls_root:?}");
    assert!(ls_root.contains("dev/"), "got {ls_root:?}");
    assert!(ls_root.contains("ram/"), "got {ls_root:?}");

    // `mount` lists every active mount.
    let mounts = sh.execute_line("mount");
    assert!(mounts.contains("/ on "), "got {mounts:?}");
    assert!(mounts.contains("/tmp on tmpfs"), "got {mounts:?}");
    assert!(mounts.contains("/dev on devfs"), "got {mounts:?}");
    assert!(mounts.contains("/ram on "), "got {mounts:?}");

    // tmpfs round-trip through the shell — proves write/read cross-mount.
    assert_eq!(sh.execute_line("write /tmp/note hello world"), "");
    let cat_tmp = sh.execute_line("cat /tmp/note");
    assert!(cat_tmp.contains("hello world"), "got {cat_tmp:?}");

    // devfs is read-only via the shell's create_file path → "read-only" map.
    let touch_dev = sh.execute_line("touch /dev/foo");
    assert!(
        touch_dev.starts_with("touch:"),
        "expected refusal, got {touch_dev:?}"
    );

    let ls_etc = sh.execute_line("ls /etc");
    assert!(ls_etc.contains("version"));
    assert!(ls_etc.contains("motd"));

    assert_eq!(sh.execute_line("cd /etc"), "");
    assert_eq!(sh.execute_line("pwd"), "/etc\n");
    assert_eq!(sh.execute_line("cd .."), "");
    assert_eq!(sh.execute_line("pwd"), "/\n");

    let cat_motd = sh.execute_line("cat /etc/motd");
    assert!(cat_motd.contains("the kernel"), "got {cat_motd:?}");

    assert_eq!(sh.execute_line("echo hi  there"), "hi there\n");
    assert_eq!(sh.execute_line("echo"), "\n");

    let nope = sh.execute_line("nope");
    assert!(nope.starts_with("willo: command not found"), "got {nope:?}");

    let bad_cd = sh.execute_line("cd /no/such");
    assert!(bad_cd.starts_with("cd: "), "got {bad_cd:?}");

    let help = sh.execute_line("help");
    assert!(help.contains("pwd"));
    assert!(help.contains("cat"));

    serial_println!("[ok] shell commands pwd/ls/cd/cat/echo/help all pass");
    exit_qemu(QemuExitCode::Success);
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    serial_println!("[FAILED] panic: {info}");
    exit_qemu(QemuExitCode::Failed);
}
