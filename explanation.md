# How Willo works

This document walks through the moving parts of the project from the
outside in: how `cargo run` turns into a booting kernel, what each module
in `kernel/src/` does, and how the integration tests close the loop. It is
the companion to `README.md` (quick start) and `description.md` (one-line
pitch). For the long-term vision and unfinished work, see
[`docs/idea.md`](docs/idea.md).

## 1. The two crates and the custom runner

The workspace has exactly two crates:

- **`kernel/`** — `no_std`, freestanding, target `x86_64-unknown-none`.
  Exposes both a library (`lib.rs`) shared with integration tests and a
  binary (`main.rs`) used as the bootable kernel.
- **`/` (the `willo` package)** — a `std` host-side helper at
  `src/main.rs`. Its sole job is to take a kernel ELF, build a BIOS disk
  image around it, and launch QEMU.

The wiring lives in `.cargo/config.toml`:

```
[target.x86_64-unknown-none]
runner = "cargo run --quiet -p willo --"
```

Whenever Cargo is invoked with `--target x86_64-unknown-none` (which the
`krun` / `ktest` aliases do), it builds the kernel, then hands the
resulting ELF path as `argv[1]` to the `willo` binary. That binary:

1. Calls `bootloader::BiosBoot::new(...).create_disk_image(...)` to build
   the boot disk.
2. Calls `build_data_disk` to format a fresh 64 MiB FAT32 image seeded
   with `/welcome.txt`, `/etc/version`, `/etc/motd`, and an empty
   `/bin/` — sized so that the `fatfs` crate auto-selects FAT32 (smaller
   volumes default to FAT16, which the kernel reader does not speak).
3. Spawns `qemu-system-x86_64` with both disks attached over IDE,
   `isa-debug-exit` wired at port `0xf4`, 256 MiB of RAM, and either a
   GUI window or `-display none -serial stdio` depending on whether the
   binary looks like a test (test artifacts live under `.../deps/`).

The runner also distinguishes test vs run binaries to choose the QEMU
display mode, and handles a few env knobs documented in the README:
`WILLO_HEADLESS`, `WILLO_GUI`, `WILLO_KEEP_DISK`, `WILLO_PERSIST`,
`WILLO_VBOX`. After a successful `fat_write` test, it shells out to
`fsck.fat` to surface any new corruption the kernel introduces.

## 2. The kernel ELF size trap

`Cargo.toml` strips kernel debuginfo even in dev builds:

```
[profile.dev.package.kernel]
debug = false
strip = "debuginfo"
```

This is load-bearing. The `bootloader 0.11.12` BIOS stages cap how much
ELF they can read off the boot disk; a 5 MB+ kernel with full debuginfo
silently fails to boot with no serial output at all. Stripping keeps
`kernel.elf` around 150 KB and means `cargo krun` and `cargo build
--release` boot identically.

## 3. Boot path

`kernel/src/main.rs` declares the entry point:

```rust
entry_point!(kernel_main, config = &kernel::BOOTLOADER_CONFIG);
```

`BOOTLOADER_CONFIG` (in `lib.rs`) asks the bootloader to identity-map all
physical memory at a dynamically-chosen virtual offset. `kernel_main`
then runs through the boot sequence:

1. `kernel::init()` — UART up, GDT/TSS loaded, IDT installed, PIC
   remapped (IRQs at 32+), interrupts enabled.
2. Read `physical_memory_offset` out of `BootInfo`, hand it to
   `memory::init` to wrap an `OffsetPageTable`, build a
   `BootInfoFrameAllocator` over the bootloader-provided memory map.
3. `allocator::init_heap` — maps a 100 KiB heap at `0x_4444_4444_0000`
   and hands it to a `linked_list_allocator`.
4. `init_framebuffer` — wraps the bootloader framebuffer in a
   `FrameBufferWriter` (16 pt Noto Sans Mono bitmap), publishes it via a
   `OnceCell<Mutex<…>>` so `print!` / `println!` work.
5. `fs::mount_root()` — picks a root filesystem (see §5).
6. `Executor::new()` — constructs the cooperative async executor and
   spawns `shell_loop(vfs)` on it; `executor.run()` never returns.

The `panic_handler` writes the panic message to serial and exits QEMU
with `QemuExitCode::Failed` so the test harness sees the failure.

## 4. Module map (`kernel/src/`)

| Module             | Role                                                                            |
| ------------------ | ------------------------------------------------------------------------------- |
| `lib.rs`           | Bootloader config, `print!`/`println!`, `init`, `exit_qemu`, `hlt_loop`.        |
| `serial.rs`        | UART 16550 driver and `serial_println!`.                                        |
| `gdt.rs`           | GDT + TSS with a dedicated double-fault stack.                                  |
| `interrupts.rs`    | IDT, PIC chaining, breakpoint, double-fault, timer, keyboard handlers.          |
| `memory.rs`        | `OffsetPageTable` setup, `BootInfoFrameAllocator` over the firmware map.        |
| `allocator.rs`     | 100 KiB linked-list heap at a fixed VA.                                         |
| `framebuffer.rs`   | Bitmap framebuffer writer with scrollback (M10).                                |
| `block.rs`         | Generic block-device trait, 28-bit LBA.                                         |
| `ata.rs`           | Polled ATA PIO driver, primary IDE bus, hardwired to drive 1 (data disk).      |
| `fs/mod.rs`        | VFS-lite: mount table, dispatch, error type.                                    |
| `fs/ram.rs`        | In-memory FS (M8 fallback, also used for tests).                                |
| `fs/fat.rs`        | Hand-rolled FAT32 reader + writer (8.3 names only).                             |
| `fs/tmp.rs`        | `tmpfs` (M10).                                                                  |
| `fs/dev.rs`        | `devfs` (M10).                                                                  |
| `task/mod.rs`      | `Task` wrapper with monotonic IDs.                                              |
| `task/executor.rs` | Cooperative executor that wakes on IRQ; run-to-completion futures.              |
| `task/simple_executor.rs` | Fallback single-task executor.                                          |
| `task/keyboard.rs` | Scancode stream + keyboard IRQ handler + scrollback key routing.                |
| `shell.rs`         | Line-oriented shell, split into a pure `execute_line` core and an async driver. |

## 5. Filesystem (`fs/`)

`fs::mount_root` returns a `Vfs` — Willo's mount table. Today it tries
the FAT data disk on the IDE primary slave first, falls back to RamFs if
the ATA probe fails. The FAT path implements 8.3 short names only: every
seeded file in `build_data_disk` is lowercase ASCII with a `≤8`-char base
and `≤3`-char extension for that reason. M10 added `tmpfs` and `devfs`
as additional mountable backends.

Shell commands surface the FS through a small CLI: `pwd`, `ls`, `cd`,
`cat`, `echo`, `clear`, `mount`, `uptime`, `touch`, `mkdir`, `rm`,
`write`, `help`. The shell separates a **pure** `Shell::execute_line`
core (no I/O, no IRQs, no globals — directly testable) from the async
driver `shell_loop`, which owns the `ScancodeStream` and mirrors output
to both the framebuffer and serial.

## 6. Async executor and IRQs

`task::executor::Executor` is a cooperative run-to-completion executor:
tasks are polled, and when they all park, the executor `hlt`s until the
next interrupt. The timer IRQ ticks a counter (used by `uptime`); the
keyboard IRQ pushes scancodes onto a `crossbeam-queue::ArrayQueue` that
the shell driver drains via `futures_util::stream::StreamExt`. There is
no preemption and no ring-3 — every task runs in kernel mode, which is
why §11 ("Userspace & ABI") of `docs/idea.md` is empty.

## 7. Tests

`kernel/tests/` contains 18 integration tests, each with `harness =
false` because they use a custom test runner that uses
`isa-debug-exit` (port `0xf4`) to signal pass/fail back to the host
runner. Topics:

- **Boot & traps** — `basic_boot`, `breakpoint`, `stack_overflow`,
  `should_panic`, `timer_interrupt`.
- **Memory** — `heap_allocation`.
- **Async** — `executor_basic`, `executor_multitask`.
- **Shell** — `shell_commands` (drives the pure core directly).
- **Block / FS** — `ata_identify`, `fat_read`, `fat_write`, `ramfs`,
  `tmpfs`, `devfs`.
- **Display** — `fb_scrollback`.
- **Userspace stub** — `ring3_jump` (groundwork for the M11 ring-3
  transition).

`cargo ktest` runs them all; the runner forces headless mode for tests
(`is_test_binary` → parent dir name is `deps`) so output goes to
`stdio` over QEMU's serial port.

## 8. What's intentionally not here

The kernel deliberately stops short of:

- Ring 3 / userspace (M11+).
- A preemptive scheduler, processes, fork/exec, signals (M12+).
- PCI/APIC, AHCI, NVMe, USB, NIC drivers, TCP/IP (M13–M14+).
- UEFI boot, ACPI, SMP (M15).
- A native journaling FS, `procfs`, `sysfs` (M16).
- Display layer, mouse, compositor, GUI toolkit (M17+).

`docs/idea.md` is the canonical roadmap and gap analysis; treat the
unchecked items there as the to-do list, not as bugs.
