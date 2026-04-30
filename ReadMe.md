# Willo

A Rust-first, `no_std` hobby kernel for x86_64, in the spirit of Philipp
Oppermann's *Writing an OS in Rust*. Willo BIOS-boots under QEMU, paints a
bitmap framebuffer, runs a cooperative async executor, mounts a FAT32 data
disk through a tiny VFS, and drops into an interactive shell. Milestones
M1–M10 are complete; the long-term aim is a desktop-class OS.

## Quick start

Requirements: a recent **nightly** Rust toolchain (the `rust-toolchain.toml`
pins the channel and the `rust-src`, `llvm-tools-preview`, `rustfmt`,
`clippy` components, plus the `x86_64-unknown-none` target) and
`qemu-system-x86_64` on `PATH`. `fsck.fat` is optional but enables the
post-`fat_write` integrity check.

```sh
cargo krun                                   # build + boot in a QEMU window
cargo ktest                                  # run all kernel integration tests
WILLO_HEADLESS=1 cargo krun                  # serial-only, no GUI
WILLO_PERSIST=1 cargo krun                   # keep the FAT data disk between runs
WILLO_VBOX=1 cargo krun                      # convert disks to VDI for VirtualBox
```

`kbuild`, `krun` and `ktest` are cargo aliases declared in
`.cargo/config.toml`. Cargo invokes the `willo` runner at `src/main.rs`,
which packages the freshly built kernel ELF into a BIOS disk image, builds
a fresh 64 MiB FAT32 data disk seeded with `/welcome.txt`, `/etc/version`,
`/etc/motd` and `/bin/`, and launches QEMU with both attached over IDE.

## Layout

```
Willo/
├── kernel/                # no_std kernel crate (lib + bin + 18 tests)
│   ├── src/               # 16 modules: memory, gdt, interrupts, framebuffer,
│   │                      # serial, allocator, ata, block, fs/, task/, shell
│   └── tests/             # integration tests run through the QEMU runner
├── src/main.rs            # std-side runner: builds the disk image + launches QEMU
├── docs/                  # design notes (idea.md is the long-form vision)
├── .cargo/config.toml     # cargo aliases + custom runner wiring
├── rust-toolchain.toml    # nightly + components + target
└── Cargo.toml             # workspace; strips kernel debuginfo on dev
```

## Shell

Type `help` once the kernel is up. Built-in commands:

```
pwd  ls  cd  cat  echo  clear  mount  uptime
touch  mkdir  rm  write  help
```

Scrollback works on the framebuffer via `PgUp` / `PgDn` / `End`.

## Runner environment overrides

| Variable          | Effect                                                          |
| ----------------- | --------------------------------------------------------------- |
| `WILLO_HEADLESS`  | Force `-display none -serial stdio` even outside tests.         |
| `WILLO_GUI`       | Force a graphical QEMU window even for tests.                   |
| `WILLO_KEEP_DISK` | Don't delete the temp `.img` files on exit.                     |
| `WILLO_PERSIST`   | Reuse `/tmp/willo-persist-data.img` so files survive reboots.   |
| `WILLO_VBOX`      | Convert disks to VDI and print VBoxManage commands; skip QEMU.  |

## Notes

- The BIOS bootloader's stages have a hard ceiling on ELF size, so dev
  builds **strip kernel debuginfo** (see `Cargo.toml`'s
  `[profile.dev.package.kernel]`). Without that, a fat ELF silently fails
  to boot with no serial output.
- The kernel's ATA driver uses legacy I/O ports `0x1F0/0x3F6`, which is why
  the runner attaches disks over IDE/PIIX4 (not AHCI) when targeting
  VirtualBox.
- `fs::fat` is FAT32 read+write but speaks 8.3 short names only — every
  seeded path is lowercase ASCII with `≤8`-char base + `≤3`-char extension.

## Roadmap

See [`docs/idea.md`](docs/idea.md) for the long-form vision and the M10 →
M20+ phased roadmap (userspace + ring 3, preemptive scheduling, PCI/APIC,
networking, UEFI/SMP, native FS, GUI compositor, package manager, …).

## License

Unlicensed hobby project; treat as personal coursework.
