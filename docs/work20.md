# work20 — M20+: stretch cluster (Wi-Fi, GPU, suspend, browser, virt, backup, eBPF, l10n, accessibility)

> Derived from `docs/idea.md` §3, §7, §10, §14, §16, §18, §19, §20.

## Goal

The "everything that makes it feel like a real desktop" cluster. This is **not one milestone** — it's a backlog of M20+ items deliberately left small so each can graduate into its own M21, M22, … as it becomes the next priority. Treat this file as the index for those follow-ups.

## Depends on

- M16 (FS/journal/snapshots), M17 (compositor + input), M18 (audio/USB), M19 (packaging) — i.e. the full desktop foundation.

## Acceptance criteria (per sub-track; all optional within M20+)

| Sub-track | Acceptance |
| --- | --- |
| Wi-Fi | mac80211-class core; one driver (`iwl-class` for QEMU + one real card); WPA2/WPA3 via userspace `willo-supplicant`. |
| GPU acceleration | virtio-gpu 3D first; one vendor (Intel iGPU) via direct rendering. |
| Suspend / resume | S3 in QEMU; lid/power button events; full driver participation (timer save/restore, USB suspend, AHCI flush). |
| Browser port | Servo or Ladybird builds + runs as a Willo-native app; loads `https://example.com`. |
| Virtualization (§18 of idea) | KVM-class hypervisor; `cloud-hypervisor`-style frontend boots a small Linux. |
| WSL-class compat | ELF + Linux syscall translator runs unmodified Ubuntu `bash`, `coreutils`, `apt-get`. |
| Backup engine (§20) | `willo-back` daemon: encrypted, dedup, incremental snapshots to local disk + WebDAV. |
| Cloud sync (§20) | Pluggable backends (WebDAV, S3); `~/Cloud/` directory mirrors live. |
| eBPF / perf tracing (§19) | In-kernel verifier + JIT (or AOT) for Willo-eBPF; `willotrace` userspace tool. |
| Localization (§16) | UTF-8 throughout; locale data; CLDR-derived; CJK rendering through fontdue; IME framework v0. |
| Accessibility (§16) | Screen reader (`willo-a11y`) over compositor protocol; high-contrast theme; full keyboard nav. |

## Task breakdown (per sub-track, summary)

### S1. Wi-Fi
- 802.11 frame parser + state machine.
- mac80211-class softmac core.
- One PCIe driver behind it.
- Userspace `willo-supplicant` (port `wpa_supplicant` or write minimal client).
- Integrate with M14 netstack as a `wlan0` interface.

### S2. GPU acceleration
- DRM/KMS (M17) gains `IOCTL_GEM_*`, `submit cmd-buf`, fences.
- virtio-gpu 3D (Venus) backend.
- One real driver (Intel) behind a feature flag.
- Mesa-equivalent userspace (start by porting Mesa).

### S3. Suspend / resume
- ACPI `_S3` sleep + wake vector.
- Driver `pm_ops { suspend, resume }`; finish for ATA, AHCI, virtio-net, xHCI, HDA, virtio-gpu.
- Persist scheduler/clock state; restore HPET monotonic delta on resume.

### S4. Browser port
- Build system: get the chosen browser's deps building on Willo (libc, libstdc++ port, JIT exemptions).
- Wire to compositor (Wayland-ish) + audio + input.
- Pin a long-term-supported version; track upstream quarterly.

### S5. Virtualization (idea §18)
- VMX/SVM bootstrap.
- VMM library + `willoc-boxes` GUI.
- VirtIO device backends shared with M14 + M17.

### S6. WSL-class Linux compat
- Linux syscall translator: tabular mapping + selective emulation for the long tail.
- Linux ELF loader differences (vDSO, robust list).
- Run a chrooted Ubuntu rootfs; full network/sound passthrough.

### S7. Backup (idea §20)
- `willo-back` daemon + CLI.
- Content-addressed chunks, BLAKE3, age-encryption.
- Targets: local, WebDAV, S3.
- Schedule + retention policy in a TOML config.

### S8. Cloud sync (idea §20)
- Mount-style integration: `~/Cloud/` is a virtual FS with per-backend subdirs.
- Conflict resolution UI in compositor.

### S9. eBPF / perf tracing
- Willo-eBPF bytecode + verifier + JIT.
- Tracepoints + kprobes + uprobes.
- `willotrace` CLI; perf events from PMU.

### S10. Localization
- ICU-class library (port + minimise).
- Locale data shipped as a `.willo` package.
- IBus-class IME framework with one engine (pinyin/kana).

### S11. Accessibility
- AT-SPI-class protocol over compositor.
- `willo-a11y` screen reader speaks via TTS (start with a prebuilt voice; full pipeline later).
- High-contrast + large-text + keyboard-only navigation.

## File mapping (per sub-track)

Each sub-track will spawn its own work/flow files (e.g. `work21.md` for Wi-Fi, `work22.md` for GPU, …). Directory layout for sub-track sources:

```
kernel/src/
├── net/wifi/                 # S1
├── gfx/dri/                  # S2
├── pm/                       # S3
├── virt/                     # S5
├── compat/linux/             # S6
└── observ/ebpf/              # S9
userspace/
├── willo-supplicant/         # S1
├── willoc-boxes/             # S5
├── willo-back/, willoc-cloud/# S7,S8
├── willotrace/               # S9
├── willo-l10n/               # S10
└── willo-a11y/               # S11
```

## Tests (smoke level)

- `wifi_assoc.rs` — associate to QEMU's `hostapd`.
- `gpu_3d_smoke.rs` — render a triangle via virtio-gpu 3D.
- `suspend_resume.rs` — S3 + wake; verify scheduler clock + open files survive.
- `linux_compat_bash.rs` — run upstream `bash` without modification.
- `vm_boot_linux.rs` — boot a tiny Linux guest under our VMM.
- `back_roundtrip.rs` — backup + restore of `~/`.
- `ebpf_basic.rs` — run a verified program counting `sys_write`.
- `l10n_render_cjk.rs` — render CJK glyph via compositor.
- `a11y_speak.rs` — screen reader pronounces a labeled button.

## Risks & open questions

- **Scope** — every sub-track is a real project. Promote one at a time to its own `workNN.md`/`flowNN.md` pair when started.
- **Browser port** — the single biggest porting risk; budget months, not weeks.
- **Wi-Fi spec drift** — keep to a small subset (WPA2/WPA3-PSK + AP modes only) for v1.
- **Suspend correctness** — every driver must implement `pm_ops` or device behaviour after resume is undefined.
- **Linux compat** — the long tail of syscalls + `/proc` shapes is huge; track which apps work, mark gaps as "expected" until users hit them.
