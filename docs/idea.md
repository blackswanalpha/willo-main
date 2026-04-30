# Willo OS — Vision & Feature Roadmap

> Status: living document. Captures the **target feature set** for evolving
> Willo from a hobby kernel into a complete desktop operating system in the
> spirit of Ubuntu and Windows. Everything below is descriptive, not yet
> committed work.

---

## 1. Vision

Willo aims to become a **Rust-first, microkernel-leaning, desktop-class
operating system** that is:

- **Open** like Ubuntu — readable source, reproducible builds, a real package
  ecosystem, and friendly to hackers.
- **Polished** like Windows — a coherent GUI, drag-and-drop, hardware support
  out of the box, signed updates, and an app store.
- **Memory-safe by construction** — written in Rust from the bootloader up,
  with `unsafe` confined to clearly audited islands (drivers, paging, IPC fast
  paths).
- **Self-hosting** eventually — the toolchain that builds Willo should run on
  Willo.

The document below lists the feature areas a "complete" desktop OS needs and,
for each, contrasts what Willo has today with what is missing. It doubles as
both a wishlist and a gap analysis.

---

## 2. Current State Snapshot (M1 – M9)

Willo currently boots on QEMU via the BIOS bootloader, prints a banner, and
drops into a minimal shell that can read files off a 64 MiB FAT32 data disk.

**Workspace layout**

```
Willo/
├── kernel/         # no_std kernel crate
│   ├── src/        # 16 modules, ~1,093 LOC
│   └── tests/      # 13 integration tests
├── src/            # std QEMU runner / disk image builder
├── Cargo.toml      # workspace
└── rust-toolchain.toml  # nightly + x86_64-unknown-none
```

**Subsystems already implemented (`kernel/src/`)**

| Module                  | Capability                                                                                                    |
| ----------------------- | ------------------------------------------------------------------------------------------------------------- |
| `memory.rs`             | Paging via bootloader-provided `OffsetPageTable`; `BootInfoFrameAllocator` over the firmware memory map.      |
| `allocator.rs`          | 100 KiB linked-list heap at `0x_4444_4444_0000`.                                                              |
| `gdt.rs`                | GDT + TSS with a dedicated double-fault stack.                                                                |
| `interrupts.rs`         | IDT, PIC chaining (IRQ 32+), timer tick counter, keyboard IRQ, breakpoint handler.                            |
| `serial.rs`             | UART 16550 driver for debug output.                                                                           |
| `framebuffer.rs`        | Bitmap framebuffer writer using `noto-sans-mono-bitmap` (16 pt).                                              |
| `task/mod.rs`           | Async `Task` wrapper with monotonic IDs.                                                                      |
| `task/executor.rs`      | Cooperative multitasking executor that wakes on IRQ; run-to-completion futures.                               |
| `task/simple_executor.rs` | Fallback single-task executor.                                                                              |
| `task/keyboard.rs`      | Scancode stream + IRQ handler.                                                                                |
| `shell.rs`              | Line-oriented shell with a pure, testable executor core (M8 quirk: clears framebuffer on overflow — no scrollback yet). |
| `block.rs`              | Generic block device trait with 28-bit LBA.                                                                   |
| `ata.rs`                | Polled ATA PIO driver, primary IDE bus, hardwired to drive 1 (data disk).                                     |
| `fs/mod.rs`             | Unified VFS-lite: switches between `RamFs` and `FatFs` at boot.                                               |
| `fs/ram.rs`             | In-memory FS used as M8 fallback.                                                                             |
| `fs/fat.rs`             | Hand-rolled FAT32 reader (no write path yet).                                                                 |

**Tests** (`kernel/tests/`): `basic_boot`, `breakpoint`, `stack_overflow`,
`should_panic`, `timer_interrupt`, `heap_allocation`, `executor_basic`,
`executor_multitask`, `ramfs`, `shell_commands`, `ata_identify`, `fat_read`,
`fat_write`.

**Boot banner** (`kernel/src/main.rs:20`): `Willo kernel — M9 disk-backed root FS`.

That is the entire substrate everything below is layered on top of.

---

## 3. Foundation: Boot & Firmware

**Target capability** — boot reliably on real and virtual modern hardware, in
both legacy and UEFI modes, with cryptographic chain-of-trust and graceful
multi-kernel selection.

**What Willo has today**

- BIOS boot via `bootloader 0.11.12`.
- Bootloader-provided framebuffer + memory map.
- Single-CPU boot only.

**What's missing**

- **UEFI bootloader** path (alongside or replacing BIOS), GPT partitioning.
- **Secure Boot** chain — signed bootloader, kernel, and `initrd`.
- **Boot menu** with kernel/recovery/firmware-update entries.
- **A/B kernel slots** for safe updates and rollback.
- **ACPI table parsing** (RSDP → XSDT → MADT/HPET/MCFG).
- **SMP / AP bring-up** — start application processors via INIT-SIPI-SIPI,
  per-CPU GDT/IDT/stacks.
- **Early boot logging** that survives the framebuffer handoff.
- **TPM 2.0 measurements** for measured boot.

---

## 4. Kernel Core

**Target capability** — a preemptive multi-process kernel with a stable
syscall ABI and the IPC primitives userland software actually expects.

**What Willo has today**

- Cooperative async executor (`task/executor.rs`).
- Single privilege level (ring 0).
- No process abstraction — only kernel-level async tasks.

**What's missing**

- **Preemptive scheduler** — priority + CFS-style fairness, work-stealing
  across cores.
- **Process / thread model** — PCBs, TCBs, address-space isolation.
- **`fork`, `exec`, `wait`, `exit`** semantics (or a spawn-style replacement).
- **Signals** or an equivalent async-event mechanism.
- **IPC primitives** — pipes, UNIX-domain sockets, shared memory, message
  queues, eventfd-equivalent.
- **Syscall ABI** — `syscall`/`sysret` fast path, ring 0/3 separation, stable
  numbered call table, vDSO for hot calls (`gettime`, etc.).
- **Loadable kernel modules** with a versioned ABI.
- **Panic recovery** — kernel oops dumps to disk + serial, optional kdump.
- **kthread workqueues** for deferred kernel work.

---

## 5. Memory Management

**Target capability** — full demand-paged virtual memory with copy-on-write,
mmap, swap, and the safety hardening modern OSes ship by default.

**What Willo has today**

- Identity-style mapping seeded by the bootloader.
- Single 100 KiB kernel heap.
- No userspace address spaces, no swap, no demand paging.

**What's missing**

- **Per-process page tables** + ASID/PCID where supported.
- **Demand paging** with anonymous + file-backed regions.
- **Copy-on-write** for `fork`.
- **`mmap` / `munmap` / `mprotect` / `madvise`**.
- **Swap to disk** with an LRU page replacement policy.
- **Page cache** unified with the FS layer.
- **Slab + buddy allocator** to replace the linked-list heap.
- **OOM killer** with a scoring policy.
- **KASLR**, **SMEP/SMAP**, **W^X**, **stack guard pages**, **kernel stack
  canaries**.
- **NUMA awareness** (stretch, multi-socket only).

---

## 6. Filesystem & Storage

**Target capability** — a pluggable VFS hosting multiple on-disk and virtual
filesystems, plus the storage drivers needed to talk to modern controllers.

**What Willo has today**

- Two-backend "VFS-lite" in `fs/mod.rs` choosing between `RamFs` and `FatFs`.
- Read-only FAT32 (`fs/fat.rs`).
- Polled ATA PIO driver only (`ata.rs`).
- A `fat_write` test stub but no actual write implementation.

**What's missing**

- **Real VFS** with a mount table, inode/dentry cache, and per-FS operations
  vtable.
- **Native journaling FS** ("`WilloFS`") — ext4-class, with crash recovery.
- **FAT32 write path** (finishes M9), then ext4 read, NTFS read for interop.
- **Virtual filesystems** — `tmpfs`, `procfs`, `sysfs`, `devfs`.
- **Permissions / ACLs / xattrs / quotas**.
- **Loopback** + a **FUSE-style** userspace FS hook.
- **Storage drivers** — AHCI, NVMe, VirtIO-blk, USB MSC.
- **Partition tables** — MBR + GPT, with a userland `parted`-equivalent.
- **`fsck`** + crash-recovery story.
- **Snapshots / subvolumes** (stretch — Btrfs/ZFS-class).
- **TRIM / discard** propagation to SSDs.

---

## 7. Device Driver Framework

**Target capability** — a uniform driver model that handles enumeration,
binding, hot-plug, power events, and IRQ routing across PCI and USB.

**What Willo has today**

- Hard-coded ATA controller and 8259 PIC.
- Direct register access from kernel modules.
- No bus abstraction.

**What's missing**

- **PCI / PCIe enumeration** with config-space access.
- **ACPI** integration — power buttons, lid switch, SCI events, PRT for IRQ
  routing.
- **APIC / MSI / MSI-X** to replace the 8259 PIC.
- **USB stack** — xHCI host controller, USB core, HID + MSC + audio classes.
- **Input subsystem** — keyboard (have) + mouse + touchpad + touchscreen +
  gamepad, with an evdev-style event interface.
- **Audio drivers** — Intel HDA + a generic AC'97 fallback.
- **GPU** — start with framebuffer + a DRM/KMS-style kernel interface, then
  per-vendor accelerated drivers (virtio-gpu first).
- **Real-time clock** + reliable wall-clock + monotonic time.
- **SMBus / I²C / GPIO** for laptop sensors.
- **Hot-plug events** delivered to userland (udev-class).

---

## 8. Networking

**Target capability** — full IP stack, sockets, and the userland services that
make a desktop "online" out of the box.

**What Willo has today** — nothing. No NIC drivers, no stack, no sockets.

**What's missing**

- **NIC drivers** — start with VirtIO-net and e1000, then RTL8139/8169 for old
  hardware.
- **TCP/IP stack** — IPv4 + IPv6, ARP/NDP, ICMP, UDP, TCP with SACK and
  congestion control, raw sockets.
- **Socket API** — POSIX-compatible `socket`/`bind`/`listen`/`accept`/
  `connect`/`send`/`recv`.
- **DNS resolver** + `/etc/hosts` + mDNS.
- **DHCP client** + static config.
- **NTP client** for time sync.
- **TLS** in userland (rustls-class), trust store with system-wide CAs.
- **Firewall hooks** — netfilter-class.
- **Wi-Fi** — mac80211-equivalent + nl80211 control plane + wpa_supplicant
  port.
- **Bluetooth** stack (stretch).
- **VPN hooks** — TUN/TAP, WireGuard kernel module.

---

## 9. Userspace & ABI

**Target capability** — a stable kernel/user boundary, an ELF runtime, and a
POSIX-ish base layer that ports of existing software can target.

**What Willo has today**

- Everything runs in ring 0; the "shell" is a kernel-internal async task.
- No syscall mechanism.
- No libc, no dynamic linker, no init.

**What's missing**

- **Syscall surface** — versioned, documented, ABI-stable.
- **ELF loader** — static + dynamic, with PT_GNU_RELRO, BTI, IFUNC.
- **Dynamic linker** (`ld-willo.so`) with `LD_LIBRARY_PATH` and `RPATH`.
- **libc** — port musl initially, write a Rust-native `willoc` long term.
- **Init system** (`willinit`, PID 1) — declarative service units, dependency
  graph, parallel start, socket activation, journaled logs (systemd-class).
- **PAM-style auth** + login.
- **Cron / timer service**.
- **Terminal emulator** with proper scrollback (fixes the M8 framebuffer
  quirk).
- **POSIX-ish shell** + coreutils — port `nushell`/`fish`, plus a busybox-like
  `box` for recovery.
- **FHS-style layout** — `/bin`, `/sbin`, `/etc`, `/usr`, `/home`, `/var`,
  `/tmp`, `/proc`, `/sys`, `/dev`.
- **Containers / namespaces** (stretch but cheap once we have processes).

---

## 10. Graphics & GUI

**Target capability** — a modern compositor-based desktop with HiDPI, smooth
animations, and a native Rust toolkit for first-party apps.

**What Willo has today**

- Bitmap framebuffer text only, single font (Noto Sans Mono 16 pt).
- No mouse, no windows, no compositor.

**What's missing**

- **DRM/KMS-style** kernel display layer with mode-setting.
- **GPU acceleration** — software fallback first, then virtio-gpu, Intel,
  AMD, NVIDIA.
- **Wayland-class compositor** (`willoc-comp`) with damage tracking.
- **Window manager** + decorations, tiling + floating.
- **Font rendering** — extend to TTF/OTF via `fontdue` or `ab_glyph`,
  subpixel + hinting.
- **Input event pipeline** — libinput-class.
- **Clipboard** + drag-and-drop + primary selection.
- **Multi-monitor** + HiDPI + per-output scaling + DPMS.
- **Theming** — light/dark/high-contrast, system accent color, animation
  prefs.
- **GUI toolkit** — port `iced` or `egui` first, then a native toolkit with
  CSS-like styling.
- **Accessibility** — screen reader, magnifier, keyboard navigation, AT-SPI
  equivalent.
- **Lock screen** + screensaver.

---

## 11. Multimedia

**Target capability** — system-wide audio routing, hardware-accelerated video
playback, and capture pipelines for camera/screen.

**What Willo has today** — nothing.

**What's missing**

- **Audio server** — PipeWire/PulseAudio-class, per-app streams, mixer,
  ducking.
- **Codec framework** — pluggable, hardware-accelerated when available
  (VA-API/VDPAU-class).
- **Video playback** pipeline — gstreamer-class.
- **Screen capture** + recording.
- **Camera / V4L2-class** capture.
- **MIDI** + pro-audio (stretch).

---

## 12. Security & Privilege

**Target capability** — defense-in-depth: hardware-enforced isolation,
least-privilege capabilities, sandboxing, and full-disk encryption — all on by
default.

**What Willo has today**

- Single ring (kernel only). No isolation between code paths.

**What's missing**

- **Ring 0 / ring 3** separation with `syscall`/`sysret`.
- **Capabilities** — POSIX-style + a finer Rust-native capability model.
- **MAC framework** — AppArmor/SELinux-class profiles, default-deny for
  daemons.
- **Sandboxing** — namespaces, seccomp-class syscall filtering, landlock.
- **Hardening** — ASLR (user + kernel), W^X, stack canaries, CFI, shadow
  stacks (CET).
- **Secure boot** chain end-to-end (bootloader → kernel → init → user
  session).
- **Full-disk encryption** — LUKS-class, with TPM unsealing.
- **Keyring** + secret service for apps.
- **Signed package verification** at install time.
- **Audit subsystem** — auditd-class.

---

## 13. Multi-user & Identity

**Target capability** — multiple concurrent users with isolated home
directories, a friendly login experience, and remote access.

**What Willo has today** — nothing; only the kernel is "the user".

**What's missing**

- **User / group database** — `/etc/passwd`, `/etc/group`, `/etc/shadow`
  format or a modern replacement with `argon2id`.
- **Login manager** — text (`getty`-class) + GUI (greeter).
- **Session manager** — DBus/systemd-logind-class.
- **`sudo` / `doas`** equivalent.
- **Home-dir provisioning** + skeleton files.
- **SSH server** in userland for remote login + SFTP.
- **Single sign-on** integrations (Kerberos/OAuth — stretch).

---

## 14. Power & Thermal

**Target capability** — a laptop user can close the lid, have it sleep, open
it later, and resume in under a second with battery drained appropriately.

**What Willo has today** — nothing.

**What's missing**

- **ACPI S-states** — S0 (working), S3 (suspend-to-RAM), S4 (hibernate), S5
  (off).
- **CPU frequency scaling** + `cpuidle` governors.
- **Battery + thermal** sensors surfaced to userland.
- **Suspend / resume** with full device-driver participation.
- **Lid switch / power button / lock** events.
- **Backlight** + brightness keys.

---

## 15. Package Management & Software Ecosystem

**Target capability** — installing, updating, and removing software is safe,
atomic, signed, and easy enough for a non-developer.

**What Willo has today** — nothing.

**What's missing**

- **Package format** — binary `.willo` archives with TOML manifest, file
  list, post-install hooks, signature.
- **Repository protocol** — content-addressed, mirror-friendly, signed
  metadata.
- **Dependency resolver** — SAT-based, à la libsolv.
- **Update daemon** — atomic A/B updates, automatic rollback on boot
  failure.
- **Sandboxed app format** — Flatpak/MSIX-class, per-app data dirs.
- **Software center** GUI.
- **Developer SDK** — `willo-pkg` CLI, manifest schema, CI templates.

---

## 16. Localization & Accessibility

**Target capability** — install once, run in any language, usable by people
with different abilities.

**What Willo has today** — ASCII-only console, English banner.

**What's missing**

- **UTF-8 everywhere** — kernel logs, file paths, shell, GUI.
- **Locale data** (CLDR-derived).
- **Input method framework** (IBus/Fcitx-class) for CJK + Indic scripts.
- **RTL text** (Arabic, Hebrew) with bidi shaping.
- **On-screen keyboard**.
- **Screen reader** + braille support.
- **High-contrast / large-text / dyslexia-friendly** themes.
- **Full keyboard navigation** for every GUI surface.
- **Captioning** support across multimedia.

---

## 17. Developer Tooling

**Target capability** — the OS is a great place to develop the OS, eventually
self-hosting.

**What Willo has today**

- Cargo aliases (`kbuild`, `krun`, `ktest`).
- 13 integration tests via `isa-debug-exit`.
- Serial debug output.

**What's missing**

- **GDB stub** over serial / virtio-console for live debugging.
- **`strace`-class** syscall tracer.
- **System profiler** — perf-class sampling.
- **Crash dump tooling** — symbolicated kernel stack traces.
- **Self-hosted Rust toolchain** (long-term) — port `rustc` + `cargo` +
  `lld`.
- **Native IDE** eventually (very long term).
- **Documentation tooling** — `mdbook`-class, runnable on Willo.

---

## 18. Virtualization & Containers

**Target capability** — run Linux and Windows guests, OCI containers, and
lightweight isolated app environments natively, in the spirit of `Hyper-V` +
`WSL2` on Windows and `KVM` + `LXD` on Ubuntu.

**What Willo has today** — nothing. No hypervisor, no namespaces, no
containers, no Linux compatibility layer.

**What's missing**

- **Type-2 hypervisor** — KVM-class, built on Intel VT-x / AMD-V; one VM per
  process, a small `kvm`-style ioctl/syscall surface.
- **Virtualization helpers** — EPT/NPT for nested paging, IOMMU passthrough
  (VT-d / AMD-Vi) for GPUs, NICs, USB devices.
- **VirtIO device backends** — `net`, `blk`, `scsi`, `gpu`, `console`,
  `balloon`, `fs`, `vsock`.
- **VMM frontend** — a `cloud-hypervisor` / Firecracker-class manager plus a
  desktop "Boxes" / "Hyper-V Manager" GUI.
- **Linux compatibility layer** — WSL2-style: ELF + Linux syscall
  translation/emulation so unmodified Ubuntu binaries run without a full
  guest kernel.
- **OCI container runtime** — Linux-namespaces-on-Willo + an OCI image
  runtime (`crun`/`youki`-class) so Docker / Podman images run natively.
- **`cgroups`-equivalent** — CPU, memory, IO, PID, and device quotas, shared
  with the scheduler in §4.
- **Sandboxing primitives** shared with §12 (Security): rootless containers,
  user namespaces, seccomp-style syscall filters.
- **Nested virtualization** (stretch).

---

## 19. Observability, Logging & Telemetry

**Target capability** — every event of interest in the kernel and userland is
captured, queryable, and survives crashes — combining the strengths of
`journald`, Windows ETW + Event Log, Linux `ftrace`/`perf`, and modern
crash-report pipelines.

**What Willo has today**

- `serial_println!` + framebuffer `println!` macros — fire-and-forget text,
  no structure, no persistence.
- No correlation IDs, no boot IDs, no crash reports beyond a panic message.

**What's missing**

- **Structured system journal** — binary, indexed, append-only on-disk log
  with `(timestamp, boot_id, priority, unit, fields…)` records (`journald` /
  Windows Event Log class).
- **Kernel ring buffer** — `dmesg`-class, with rate limiting and priority
  filters; survives soft reboots via reserved memory.
- **Tracing framework** — static tracepoints + dynamic probes (eBPF / DTrace
  class), per-CPU lock-free ring buffers, userspace `bpftrace`-class CLI.
- **Performance counters** — PMU sampling, hardware perf events,
  flamegraph-friendly profiles, exported to a userland `perf`-like tool.
- **Crash reporting** — kernel oops + userspace coredumps captured,
  symbolicated, opt-in upload (`Apport` / Windows Error Reporting class).
- **Health metrics service** — CPU / memory / disk / network / temperature
  counters exposed over a query API, consumable by §18 system monitor.
- **Audit log** feeding §12 (Security) — login events, privilege escalations,
  policy denials.
- **Privacy-preserving telemetry** — opt-in, transparent payload, off by
  default; absolutely no PII; reproducible from source what is sent.
- **Live debugger** — `kdb` / `crash`-style attached over serial or
  `virtio-console`, integrated with the GDB stub from §17.

---

## 20. Backup, Sync & Recovery

**Target capability** — losing a disk, accidentally `rm`-ing a file, a bad
update, or moving to a new machine never costs the user data, mirroring what
Time Machine, File History, and OneDrive/iCloud achieve, but on open
protocols.

**What Willo has today** — nothing.

**What's missing**

- **Native snapshots** in `WilloFS` (§6) — copy-on-write subvolumes,
  scheduled retention, atomic restore.
- **System restore points** — pre / post-update kernel + config snapshots
  with one-click rollback from the boot menu (§3) and the package manager
  (§15).
- **File-level history** — versioned home directories so any file can be
  rewound (Time Machine / File History class).
- **Backup engine** — incremental, encrypted, deduplicated, content-addressed;
  targets local disk, network share, and cloud (`restic` / `borg` class).
- **Cloud sync framework** — pluggable backends (WebDAV, S3, IMAP for mail,
  IPFS, vendor APIs); CRDT-style merge for shared docs.
- **Online accounts service** — single store of OAuth tokens and app
  passwords feeding sync, mail, calendar (GNOME Online Accounts class).
- **Recovery environment** — a minimal bootable Willo image with repair
  tools, fsck, network drivers, and shell, on the same disk or a USB.
- **Migration assistant** — import a user from another Willo box, an Ubuntu
  partition, or a Windows install (settings, files, app list).

---

## 21. Application Suite (Day-One Apps)

To feel like a real desktop OS, Willo needs a baseline set of bundled apps.
Targets:

- **Terminal emulator** with tabs, true-color, Unicode.
- **File manager** with previews, batch operations, network mounts.
- **Text editor** — `helix`/VS-Code-class.
- **Image viewer** + basic editor.
- **Web browser** — port `servo` or `ladybird`; this is the single largest
  porting effort and gates "real desktop" credibility.
- **Email client** + calendar.
- **Settings panel** unifying user, network, display, sound, power, updates.
- **System monitor** — processes, CPU, memory, disk, net.
- **Calculator**, **media player**, **screenshot tool**, **archive
  manager**.

---

## 22. Suggested Phased Roadmap (M10 → M20+)

This is **a suggestion**, not a commitment. Each milestone is one focused
push that lands tests + docs.

| Milestone | Focus                                                                                          |
| --------- | ---------------------------------------------------------------------------------------------- |
| **M10**   | Finish FAT32 write path; add framebuffer scrollback; `tmpfs` + `devfs`.                         |
| **M11**   | Userspace foundations — ring 3, syscall ABI, ELF loader, first userspace `init` + `sh`.        |
| **M12**   | Preemptive scheduler, processes/threads, fork/exec/wait, signals, basic IPC.                   |
| **M13**   | PCI enumeration, APIC, AHCI driver, swap, slab allocator.                                       |
| **M14**   | NIC driver (virtio-net) + TCP/IP stack + DHCP + DNS + first network app (e.g. `ping`, `curl`). |
| **M15**   | UEFI boot, ACPI parsing, SMP bring-up.                                                          |
| **M16**   | Native journaling FS (`WilloFS`); `procfs` + `sysfs`; **snapshots + structured system journal MVP** (§19, §20). |
| **M17**   | Display layer (KMS), mouse, compositor MVP, terminal-as-window-app.                            |
| **M18**   | Audio (HDA), USB stack (xHCI + HID + MSC).                                                      |
| **M19**   | Package manager + repo + signed updates.                                                        |
| **M20+**  | Wi-Fi, GPU acceleration, suspend/resume, browser port, accessibility, l10n; **virtualization + WSL-class compat (§18)**, **backup engine + cloud sync (§20)**, **eBPF + perf tracing (§19)**. |

---

## 23. Out of Scope (for now)

These are explicitly **not** desktop-OS goals and should not bleed into the
roadmap unless the vision changes:

- Mobile / touch-first form factors.
- Hard real-time kernel guarantees.
- Mainframe / multi-tenant server scale.
- Cluster / distributed-OS features.

---

## 24. References

- Philipp Oppermann's "Writing an OS in Rust" blog series — the structural
  basis of M1–M9.
- [OSDev Wiki](https://wiki.osdev.org/) — the canonical reference for x86
  internals.
- *Operating Systems: Design and Implementation* (Tanenbaum, MINIX 3 book).
- The Linux kernel `Documentation/` tree — for contracts every modern OS
  ends up needing to honour.
- ReactOS architecture docs — for Windows-compat lessons.
- [Redox OS](https://www.redox-os.org/) book — closest peer in spirit
  (Rust + microkernel + desktop ambition).
- [Wayland Book](https://wayland-book.com/) — for the compositor design in
  §10.
- [Phil Karlton-style ABI stability writing on Linux syscalls] — for the
  stable boundary in §9.

---

*End of document. Edit freely as the vision sharpens.*
