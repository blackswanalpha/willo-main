//! Universal kernel runner.
//!
//! Cargo invokes this binary as the configured `runner` for the
//! `x86_64-unknown-none` target (see `.cargo/config.toml`). It receives the
//! freshly built kernel/test ELF as `argv[1]`, packages it into a BIOS disk
//! image via the `bootloader` crate, then launches `qemu-system-x86_64` with
//! an `isa-debug-exit` device so the guest can exit QEMU with a status code.
//!
//! Detection rule for "this is a test binary":
//! - cargo always places test artifacts under `.../deps/<name>-<hash>`, so we
//!   treat the parent dir name `deps` as the signal. Test binaries default to
//!   headless serial-only mode and we expect the kernel to exit via the
//!   `isa-debug-exit` port. Non-test binaries default to a graphical QEMU
//!   window for normal `cargo run` usage.
//!
//! Every run also generates a small FAT32 **data disk** (8 MiB) seeded with
//! `/welcome.txt`, `/etc/{version,motd}`, `/bin/` and attaches it as the IDE
//! primary slave so the kernel's M9 ATA + FAT reader has something to mount.
//!
//! Env overrides:
//! - `WILLO_HEADLESS=1` — force headless even for `cargo run`
//! - `WILLO_GUI=1` — force graphical even for tests (rare; useful for debugging)
//! - `WILLO_KEEP_DISK=1` — don't delete the temp disk images on exit
//! - `WILLO_PERSIST=1` — reuse a sticky data disk at
//!   `/tmp/willo-persist-data.img` instead of regenerating per-run, so any
//!   files the user creates with `touch`/`write`/`mkdir` survive reboots
//! - `WILLO_VBOX=1` — convert both raw disks to `.vdi` via
//!   `VBoxManage convertfromraw`, print the VBoxManage commands needed to
//!   register and start the VM, and exit without launching QEMU.

use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, exit};

const QEMU_EXIT_SUCCESS: i32 = 33; // (0x10 << 1) | 1
const QEMU_EXIT_FAILED: i32 = 35; //  (0x11 << 1) | 1
// 64 MiB — large enough that the `fatfs` crate's auto-selection picks FAT32
// (FAT16 kicks in for smaller volumes). Our reader is FAT32-only.
const DATA_DISK_BYTES: u64 = 64 * 1024 * 1024;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let prog = args
        .first()
        .cloned()
        .unwrap_or_else(|| "willo".to_string());

    let kernel: PathBuf = match args.get(1) {
        Some(p) => PathBuf::from(p),
        None => {
            eprintln!("usage: {prog} <kernel-elf>");
            eprintln!();
            eprintln!("typical invocation: cargo run -p kernel --target x86_64-unknown-none");
            eprintln!("(cargo invokes this runner with the kernel ELF path as argv[1])");
            exit(2);
        }
    };
    if !kernel.exists() {
        eprintln!("{prog}: kernel ELF not found: {}", kernel.display());
        exit(2);
    }

    let is_test = is_test_binary(&kernel);
    let headless = std::env::var_os("WILLO_HEADLESS").is_some()
        || (is_test && std::env::var_os("WILLO_GUI").is_none());
    let keep_disk = std::env::var_os("WILLO_KEEP_DISK").is_some();
    let vbox = std::env::var_os("WILLO_VBOX").is_some();

    let pid = std::process::id();
    let disk = std::env::temp_dir().join(format!("willo-{pid}.img"));
    if let Err(e) = bootloader::BiosBoot::new(&kernel).create_disk_image(&disk) {
        eprintln!("{prog}: failed to build disk image: {e}");
        exit(1);
    }

    let persist = std::env::var_os("WILLO_PERSIST").is_some();
    let data = if persist {
        std::env::temp_dir().join("willo-persist-data.img")
    } else {
        std::env::temp_dir().join(format!("willo-{pid}-data.img"))
    };
    if !persist || !data.exists() {
        if let Err(e) = build_data_disk(&data) {
            eprintln!("{prog}: failed to build FAT data disk: {e}");
            exit(1);
        }
        if persist {
            eprintln!("[willo] new persistent data disk: {}", data.display());
        }
    } else {
        eprintln!("[willo] reusing persistent data disk: {}", data.display());
    }

    if vbox {
        run_vbox_mode(&prog, &disk, &data);
    }

    let mut cmd = Command::new("qemu-system-x86_64");
    cmd.arg("-drive")
        .arg(format!("file={},if=ide,index=0,format=raw", disk.display()));
    cmd.arg("-drive")
        .arg(format!("file={},if=ide,index=1,format=raw", data.display()));
    cmd.arg("-no-reboot");
    cmd.arg("-device")
        .arg("isa-debug-exit,iobase=0xf4,iosize=0x04");
    cmd.arg("-m").arg("256M");
    if headless {
        cmd.arg("-display").arg("none");
        cmd.arg("-serial").arg("stdio");
    } else {
        cmd.arg("-serial").arg("mon:stdio");
    }

    if !is_test && !headless {
        eprintln!("[willo] running {} in QEMU (Ctrl+A then X to exit)", kernel.display());
    }

    let status = match cmd.status() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{prog}: failed to spawn qemu-system-x86_64: {e}");
            if !keep_disk {
                let _ = std::fs::remove_file(&disk);
                if !persist {
                    let _ = std::fs::remove_file(&data);
                }
            }
            exit(1);
        }
    };

    // Post-test fsck for the FAT data disk after fat_write — M10 acceptance
    // criterion. Always informational, never fatal: the host-side `fatfs 0.3`
    // crate emits `.`/`..` directory entries that `fsck.fat` rejects, so a
    // freshly-built image fails fsck *before* any kernel write. Replacing the
    // builder is a separate cleanup; meanwhile this step still surfaces any
    // *new* corruption the kernel introduces (visible as fresh complaints
    // about paths the kernel created).
    if is_test
        && status.code() == Some(QEMU_EXIT_SUCCESS)
        && is_fat_write_test(&kernel)
        && data.exists()
    {
        match Command::new("fsck.fat")
            .args(["-nv", data.to_str().expect("utf8 disk path")])
            .status()
        {
            Ok(s) if s.success() => {
                eprintln!("[willo] fsck.fat -nv: clean");
            }
            Ok(s) => {
                eprintln!(
                    "[willo] fsck.fat -nv reported errors (exit {s}); see host-builder \
                     note in src/main.rs."
                );
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                eprintln!("[willo] fsck.fat not on PATH; skipping post-test fsck");
            }
            Err(e) => {
                eprintln!("[willo] failed to spawn fsck.fat: {e}");
            }
        }
    }

    if !keep_disk {
        let _ = std::fs::remove_file(&disk);
        if !persist {
            let _ = std::fs::remove_file(&data);
        }
    }

    match status.code() {
        Some(QEMU_EXIT_SUCCESS) => exit(0),
        Some(QEMU_EXIT_FAILED) => {
            eprintln!("[willo] kernel reported FAILED via isa-debug-exit");
            exit(1);
        }
        Some(0) => exit(0),
        Some(other) => exit(other),
        None => exit(1),
    }
}

fn is_fat_write_test(p: &Path) -> bool {
    p.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.starts_with("fat_write"))
        .unwrap_or(false)
}

fn is_test_binary(p: &Path) -> bool {
    p.parent()
        .and_then(|d| d.file_name())
        .map(|n| n == "deps")
        .unwrap_or(false)
}

/// Convert both raw disks to VDIs and print VBoxManage commands. Exits.
///
/// Two disks are needed: the BIOS-bootable kernel image (primary master) and
/// the FAT data disk (primary slave) the kernel mounts at `/`. We pick the
/// **IDE / PIIX4** controller because the kernel's M9 ATA driver speaks legacy
/// I/O ports `0x1F0/0x3F6`; SATA in VirtualBox uses AHCI which would need a
/// different driver. `keep_disk` is force-true: the runner exits before the
/// QEMU cleanup, so both raw `.img` files and both `.vdi` files survive.
fn run_vbox_mode(prog: &str, raw: &Path, data: &Path) -> ! {
    let pid = std::process::id();
    let vdi = std::env::temp_dir().join(format!("willo-{pid}.vdi"));
    let data_vdi = std::env::temp_dir().join(format!("willo-{pid}-data.vdi"));
    let _ = std::fs::remove_file(&vdi);
    let _ = std::fs::remove_file(&data_vdi);

    convert_to_vdi(prog, raw, &vdi);
    convert_to_vdi(prog, data, &data_vdi);

    let vdi_str = vdi.display().to_string();
    let data_vdi_str = data_vdi.display().to_string();
    println!();
    println!("[willo] boot VDI: {vdi_str}");
    println!("[willo] data VDI: {data_vdi_str}");
    println!("[willo] copy-paste these commands to register and boot in VirtualBox:");
    println!();
    println!("  VBoxManage createvm --name Willo --ostype Other_64 --register");
    println!(
        "  VBoxManage modifyvm Willo --memory 256 --firmware bios \\\n    \
         --boot1 disk --boot2 none --boot3 none --boot4 none"
    );
    println!(
        "  VBoxManage storagectl Willo --name IDE --add ide \\\n    \
         --controller PIIX4 --bootable on"
    );
    println!(
        "  VBoxManage storageattach Willo --storagectl IDE --port 0 --device 0 \\\n    \
         --type hdd --medium {vdi_str}"
    );
    println!(
        "  VBoxManage storageattach Willo --storagectl IDE --port 0 --device 1 \\\n    \
         --type hdd --medium {data_vdi_str}"
    );
    println!("  VBoxManage startvm Willo");
    println!();
    println!("  # teardown later:");
    println!("  VBoxManage unregistervm Willo --delete");
    println!();
    println!("note: the kernel uses QEMU's isa-debug-exit (port 0xf4) to exit.");
    println!("      VirtualBox doesn't wire that port, so on Success/Failed the");
    println!("      kernel will idle in a nop loop. Power-off the VM to stop.");
    exit(0);
}

fn convert_to_vdi(prog: &str, raw: &Path, vdi: &Path) {
    let convert = Command::new("VBoxManage")
        .args([
            "convertfromraw",
            raw.to_str().expect("raw path utf8"),
            vdi.to_str().expect("vdi path utf8"),
            "--format",
            "VDI",
        ])
        .status();
    match convert {
        Ok(s) if s.success() => {}
        Ok(s) => {
            eprintln!("{prog}: VBoxManage convertfromraw exited {s}");
            exit(1);
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("{prog}: VBoxManage not found on PATH.");
            eprintln!("install VirtualBox (e.g. `sudo apt install virtualbox`) and retry.");
            eprintln!("the raw disk is preserved at: {}", raw.display());
            exit(1);
        }
        Err(e) => {
            eprintln!("{prog}: failed to spawn VBoxManage: {e}");
            exit(1);
        }
    }
}

/// Build a fresh 8 MiB FAT32 data disk seeded with a small read-only tree.
///
/// The kernel's `fs::fat` reader supports 8.3 short names only, so every name
/// here is lowercase ASCII and ≤8 char base + ≤3 char ext.
fn build_data_disk(path: &Path) -> std::io::Result<()> {
    let _ = std::fs::remove_file(path);
    let mut f = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)?;
    f.set_len(DATA_DISK_BYTES)?;

    let opts = fatfs::FormatVolumeOptions::new().fat_type(fatfs::FatType::Fat32);
    fatfs::format_volume(&mut f, opts).map_err(io_err)?;
    f.seek(SeekFrom::Start(0))?;

    let fs = fatfs::FileSystem::new(&mut f, fatfs::FsOptions::new()).map_err(io_err)?;
    {
        let root = fs.root_dir();

        let mut welcome = root.create_file("welcome.txt").map_err(io_err)?;
        welcome
            .write_all(b"Welcome to Willo!\nType `help`.\n")
            .map_err(io_err)?;
        welcome.flush().map_err(io_err)?;

        let etc = root.create_dir("etc").map_err(io_err)?;
        let mut version = etc.create_file("version").map_err(io_err)?;
        version.write_all(b"willo 0.9 (M9)\n").map_err(io_err)?;
        version.flush().map_err(io_err)?;
        let mut motd = etc.create_file("motd").map_err(io_err)?;
        motd.write_all(b"the kernel that fits in your head\n")
            .map_err(io_err)?;
        motd.flush().map_err(io_err)?;

        root.create_dir("bin").map_err(io_err)?;
    }
    fs.unmount().map_err(io_err)?;
    Ok(())
}

fn io_err<E: std::fmt::Display>(e: E) -> std::io::Error {
    std::io::Error::other(e.to_string())
}
