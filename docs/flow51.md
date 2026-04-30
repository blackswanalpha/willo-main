# flow51 — cross-cutting: end-to-end boot sequence (firmware → user session)

> Cross-cutting flow indexed across §3, §4, §6, §10, §13, §15, §17, §19. Touches M10–M50.

This is **not a milestone flow** — it is the integrated picture of what happens between power-on and a logged-in desktop, citing every milestone that contributes a step. Use it as a reference when reasoning about boot regressions or cold-start performance.

## End-to-end timeline

```
T+0 ms   firmware POST + UEFI variable load              [§3]
T+~50ms  shim secure-boot verifies bootloader            [§47]
T+~80ms  bootloader: AVB-class verify vbmeta             [§47]
T+~100ms boot menu (5s timeout, default first)           [§47]
T+~110ms TPM PCR 0/2/4/7 extended                        [§38, §47]
T+~120ms slot picked; kernel + initrd loaded             [§47]
T+~150ms TPM PCR 8 (cmdline) + PCR 9 (initrd) extended   [§47]
T+~170ms jump to kernel
         GDT/IDT/paging/heap (M1-M7)                     [M-base]
         framebuffer init + scrollback                   [M10]
         ATA / AHCI / NVMe drivers come up               [M13]
         FS roots: WilloFS / FAT (with crypt layer)      [M16, M38]
         PCR 10/11 extended (modules + root FS hash)     [§47]
T+~600ms kernel ring-3 transition                        [M11]
         init: PID 1 (willinit)                          [M11, M37]
T+~650ms udev-class hotplug enumerates devices           [M48 hooks]
T+~700ms cgroups v2 root populated                       [M41]
T+~750ms /dev mounts; /tmp tmpfs; /proc procfs           [M10, M16]
T+~800ms early services: journald (§16), networkd (§14)  [M14, M16]
T+~900ms wlan0 / eth0 up; DHCP                           [M14, M21]
T+~1.0s  systemd-class targets ramping in parallel       [M37]
         willortkit (§44 if pro-audio)
         willologind (§37)
         willoc-secrets (§38)
         willoc-accounts (§28)
         willopipe (§40)
T+~1.2s  display target: KMS + compositor               [M17, M22]
T+~1.4s  willogreet appears (or willologin on tty1)     [M37]
T+~1.5s  user picks; PAM auth (argon2id)                 [M37, M38]
         pam_systemd registers session                   [M37]
         /home/$user FDE unlocked (per-user key)         [M38]
T+~1.7s  user willoc-comp + a11y bus + audio session    [M17, M31, M18]
T+~2.0s  autostart: willoc-cloud (§28), willomail-sync   [M28, M35]
         §31 a11y daemons if user.preference set
T+~2.2s  desktop ready; idle
T+~30s   willobootctl mark-good (§47)                    [M47, M19]
```

## Component handoff map

```
firmware  --[boot_params]--> bootloader
bootloader --[kernel + initrd + cmdline]--> kernel
kernel    --[fs_root, /init exec]--> userspace
userspace --[DBUS_SESSION, XDG_*]--> compositor + apps
compositor --[wl_display socket]--> per-user clients
```

## What each milestone contributes

| Milestone | Role at boot |
| --- | --- |
| M10 | early framebuffer + tmpfs + devfs |
| M11 | ring 3 + ELF + first userspace |
| M13 | PCI + APIC + AHCI/NVMe |
| M14 | netstack + DHCP + DNS |
| M15 | UEFI + ACPI parsing + SMP |
| M16 | WilloFS root + journal early |
| M17 | display target (compositor up) |
| M19 | package metadata + boot mark-good |
| M21 | wlan0 if Wi-Fi-only |
| M22 | DRM/GEM/dma-buf for compositor |
| M23 | cpufreq + battery + lid handlers wired |
| M28 | online-accounts daemon |
| M37 | login + session + sudo + sshd |
| M38 | LUKS unseal at boot, per-user FDE on login |
| M39 | MAC + seccomp + Landlock + audit |
| M40 | willopipe + audio mixer + screencast |
| M41 | cgroups + namespaces ready for containers |
| M44 | optional pro-audio path (RT, threadirq) |
| M45 | module loader fires on hotplug |
| M47 | A/B slots + measured boot + rollback |
| M48 | hwmon + fancontrol come up |

## Key parallelism

- M14 net + M22 GPU + M37 session bring-up overlap; only the compositor's wl_display socket is the dependency for user clients.
- M28/M35 sync daemons defer until network reachable to avoid wasted retries.
- §44 RT mode delays §28/§35 startup until audio thread is RT-scheduled.

## Failure paths

- **Slot retry exhausts** → §47 rollback; previous kernel boots; user notified via journal.
- **Compositor fails** → fall back to text getty; user can still log in.
- **PAM unlock fails** → loop on greeter; never auto-promote.
- **TPM PCR mismatch** → §38 fallback to passphrase; banner offers rebind.
- **Journal disk full** at early boot → ring buffer in RAM; surfaces "journal degraded" in monitor.

## Tunable knobs

- Boot menu timeout: bootloader EFI var.
- Parallel target startup: per-unit ordering hints in `/etc/willo/units/*.toml`.
- Pro-audio mode toggle: `/etc/willo/audio.toml` (cold-boot only).
- KASLR: `randomize_kernel_base` cmdline (default on).
- Secure-boot enforcing vs permissive: §47 build flag.
