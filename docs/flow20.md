# flow20 — M20+: architecture & runtime flows (stretch cluster index)

> Derived from `docs/idea.md` §3, §7, §10, §14, §16, §18, §19, §20.

> This file is an **index of flows** for the M20+ sub-tracks listed in
> `work20.md`. Each sub-track will graduate into its own `flowNN.md` once
> promoted to a milestone. Diagrams below are sketches to anchor the
> integration points.

## Top-level system after M20+

```
                             user apps
                                │
                                v
              +-----------------+-----------------+
              |              compositor          |       (a11y bus)
              +---+-----+-----+-----+-----+------+
                  |     |     |     |     |
                  v     v     v     v     v
              [audio][fs][net][input][gpu-accel]
                  │
                  v
              +-----+
              |kernel: drm+gem+fences (S2), pm_ops (S3),
              | wifi mac80211 (S1), vmx (S5), ebpf (S9), ...|
              +----------------------------------------------+
                  |
                  v
              hardware
```

## S1. Wi-Fi flow

```
nl-class control plane:
  willo-supplicant -- ioctl/netlink-class --> mac80211 core
                                              │
                                              v
                                          PCIe Wi-Fi driver
                                              │
                                              v
                                            radio

data plane:
  netstack (M14) ── tx/rx skb ── mac80211 ── driver ── radio
```

## S2. GPU 3D flow

```
app (Vulkan/GL) ──> Mesa ──> DRM submit IOCTL
                                   │
                                   v
                              kernel DRM (gem buffers, fences)
                                   │
                                   v
                              virtio-gpu 3D / Intel iGPU
```

## S3. Suspend / resume sequence

```
trigger (lid close / button / timeout)
  ├─ pm.freeze_userspace                   # SIGSTOP-ish
  ├─ for dev in dev_tree DFS:
  │     dev.pm_ops.suspend()
  ├─ save cpu state (gs, msrs, idt, gdt, cr3, lapic timers)
  ├─ ACPI._S3 enter
       ↓
       ↑ wake (vector returns to kernel)
  ├─ restore cpu state
  ├─ for dev in dev_tree DFS reverse:
  │     dev.pm_ops.resume()
  ├─ adjust HPET-monotonic delta for elapsed wallclock
  └─ pm.thaw_userspace
```

## S4. Browser

```
willo-browser (Servo/Ladybird port)
  ├─ uses libc + libstdc++ ports
  ├─ talks compositor (wayland-ish) for windows
  ├─ talks audio for media
  ├─ talks netstack (TLS via rustls userland)
  └─ uses GPU via Mesa (S2)
```

## S5. Virtualization (idea §18)

```
willoc-boxes (GUI)  ── VMM API ─> willo-vmm (userspace)
                                       │
                                       v
                                 /dev/willo-vmx
                                       │
                                       v
                              kernel KVM-class
                                       │
                                       v
                              VMX/SVM hardware

guest devices: virtio-net, virtio-blk, virtio-gpu, virtio-console
host hand-off: shared memory + eventfd-class signal
```

## S6. WSL-class Linux compat

```
linux ELF binary
  ├─ loaded by elf::load (M11) with linux-aux mode
  ├─ syscall: int 0x80 / syscall instruction
  │     ├─ kernel routes to compat::linux::dispatch
  │     ├─ mapped → native Willo syscall
  │     └─ unmapped → emulated or ENOSYS
  └─ /proc, /sys shapes provided by translation FS
```

## S7. Backup

```
schedule fires:
  willo-back snapshot @snap-now
  diff_blocks(@snap-prev, @snap-now)
  for each new chunk:
     hash = blake3(chunk)
     if hash not in remote.index:
        encrypted = age::encrypt(chunk, key)
        remote.put(hash, encrypted)
  remote.put_index(snapshot_id → [hashes])
```

## S8. Cloud sync

```
~/Cloud/dropbox/  ←→ dropbox backend
~/Cloud/icloud/   ←→ icloud backend
                       │
                       v
                  cloud-sync daemon
                  (watches inotify-class events on local,
                   long-polls remote, applies CRDT merge)
```

## S9. eBPF / perf

```
user
  willotrace ── load .bpf.o ──> verifier ──> JIT/AOT ──> attach point
                                                              │
                                                              v
                                            tracepoint / kprobe / uprobe
                                                              │
                                                              v
                                                       per-CPU map
                                                              │
                                              userland tail ──┘
```

## S10. Localization

```
text rendering:
  string ──> ICU bidi/shaper ──> font selection ──> fontdue rasteriser
                                       │
                                       v
                              compositor surface
input:
  key event ──> IME engine ──> composed text ──> client surface
```

## S11. Accessibility

```
compositor publishes A11y tree (label, role, value) per surface
  │
  v
willo-a11y screen reader subscribes
  │
  v
TTS engine ──> audio core (M18)
keyboard nav: bypass focus; reads element under "a11y cursor"
```

## Failure paths (cross-cutting)

- Sub-track lands incomplete → keep behind a feature flag in `Cargo.toml` so M19 packaging never ships partial features.
- Driver suspend bug → mark device "no-suspend" (PM gating); user warned via journal.
- VM escape (S5) → security audit before enabling on by-default; until then, opt-in via `/etc/willo/virt.toml`.
- Browser port goes stale → quarterly refresh task scheduled via §15 update daemon.
