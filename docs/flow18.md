# flow18 — M18: architecture & runtime flows

> Derived from `docs/idea.md` §7, §11 and the work18 plan.

## Component map

```
   userspace
   +----------+   +----------+   +-------------+
   | aplay    |   | other    |   | willoc-aud  |
   +----+-----+   +----+-----+   +------+------+
        |              |                |
        +-- pcm wire --+                |
                                 (UNIX sock /run/willoc/aud)
                                        |
                                        v
   kernel                          +---------+
                                   |  audio  |
                                   |  core   |
                                   +----+----+
                                        |
                                        v
                                   +---------+
                                   |  HDA    |
                                   +---------+

   USB:
   xHCI ── core ── hid  ──> input core ──> /dev/input/event*
            │
            └── msc ──> block ──> VFS ──> /media/<label>
```

## USB device attach

```
xhci IRQ: port-status-change
  ├─ usb_core.port_reset(port)
  ├─ usb_core.address_device(port) → addr
  ├─ usb_core.get_descriptors(addr) → device, config, interfaces
  ├─ for each interface:
  │     bind_class_driver(interface)   # hid | msc | other
  ├─ sysfs.add(/sys/bus/usb/devices/<addr>)
  └─ journal: "usb attach addr=N vid=... pid=..."
```

## HID input flow

```
xhci interrupt-IN ring → report buffer
  ├─ hid.parse_report(buf)
  │     keyboard: modifiers + 6 keycodes
  │     mouse:    buttons + x,y,wheel
  └─ input.push_event({dev, code, value, ts=hpet_now()})
        → /dev/input/eventN ring
```

## MSC read

```
block.read(lba, n_blocks, buf)
  └─ msc.cbw(SCSI READ(10), lba, n_blocks)
     msc.bulk_in(buf)
     msc.csw_check()
```

## HDA play path

```
willoc-aud.write(stream, frames)
  ├─ resample if needed
  ├─ mix into substream's cyclic buffer
  └─ HDA DMA reads buffer → codec → DAC → speakers

stream config:
  CORB.send(SET_FORMAT)
  CORB.send(SET_AMP_GAIN)
  HDA.start(stream)
  IRQ on buffer half/complete → wake up audio core
```

## Audio mixer graph

```
   client A.pcm ─┐
                 ├─> resample ─> mix ─> [hardware substream] ─> codec ─> DAC
   client B.pcm ─┘
   sysbeep      ─┘
```

Per-node: gain, mute, channel-map.

## Hotplug events

```
xhci port-status: device-disconnected
  ├─ usb_core.disconnect(addr)
  │     unbind class drivers
  │     mark interfaces gone
  │     sysfs.remove(/sys/bus/usb/devices/<addr>)
  ├─ for MSC: VFS.unmount("/media/<label>") if mounted
  └─ journal: "usb detach addr=N"
```

## Audio client protocol

```
client → server:
  HELLO  { sample_rate, channels, format }
  ACK    { server-sample-rate; resampler ok }
  loop:
    DATA  { frames }   # blocking when mixer full
  CLOSE
```

## Failure paths

- xHCI command timeout (5 s) → controller reset; in-flight transfers fail with `EIO`.
- HID report descriptor too complex (M18 stub) → log + skip; fall back to boot protocol.
- MSC stall (CSW with status=2) → ClearFeature(HALT) on bulk-in; retry once.
- HDA xrun (buffer underrun) → fill with silence, log to journal, increase scheduler priority for audio thread.
- USB drive yanked mid-write → outstanding writes return `EIO`; FS marks superblock dirty (next mount forces fsck).
