# work43 — M43: Bluetooth (HCI + L2CAP + GATT, profiles A2DP/HFP/HID, BLE)

> Derived from `docs/idea.md` §8 (Networking — Bluetooth stack).

## Goal

Bring a usable Bluetooth 5.x + BLE stack to Willo. Kernel hosts **HCI**, **L2CAP**, and **SCO/ISO** transport; userspace `willobtd` (BlueZ-class daemon) implements **GATT**, **SDP**, and the standard profiles **A2DP** (audio sink/source), **HFP** (hands-free), and **HID** (input). Pairing supports Just Works / Numeric Comparison / Passkey.

## Depends on

- **M13** — USB controller (most BT chips are USB).
- **M18** — audio core (A2DP/HFP).
- **M30** — input subsystem (HID).
- **M39** — sandbox profile for `willobtd`.

## Acceptance criteria

- [ ] HCI USB driver enumerates a controller; `/dev/willobt/hci0` opens.
- [ ] `willobtctl scan on` discovers devices; advertisements parsed.
- [ ] Pairing with a BLE device using Numeric Comparison (6-digit) succeeds; LTK persisted.
- [ ] HID keyboard pairs and types into focused app via §30 input pipeline.
- [ ] A2DP sink: phone streams audio; Willo plays via §18 mixer.
- [ ] A2DP source: Willo plays to a BT speaker.
- [ ] HFP: place a SCO call sink/source through §18 audio.
- [ ] BLE GATT client reads a battery-level characteristic.
- [ ] `kernel/tests/hci_loopback.rs` and `userspace/willobtd/tests/pair_numeric.rs` pass.

## Task breakdown

### T1. HCI transport — `kernel/src/bt/hci.rs`
- USB transport + UART (uart-class for embedded).
- Command/event packet codec; SCO data channel.
- `/dev/willobt/hci0` char dev for userspace.

### T2. L2CAP — `kernel/src/bt/l2cap.rs`
- PSM channels + CID; segmentation/reassembly; ERTM (enhanced retransmission) optional.
- BLE attribute protocol PSM (0x0004).

### T3. SCO/ISO — `kernel/src/bt/sco.rs`
- Synchronous-channel link for HFP audio; ISO for LE Audio (BAP) — BAP deferred.

### T4. GATT — `userspace/willobtd/gatt/`
- Attribute DB (handle → (UUID, value, perms)).
- ATT client + server; long writes via prepare/exec.

### T5. SDP — `userspace/willobtd/sdp/`
- Service Discovery Protocol record DB; queries for HFP/A2DP/HID UUIDs.

### T6. Profiles — `userspace/willobtd/profiles/{a2dp,hfp,hid}.rs`
- A2DP: SBC mandatory, AAC optional; AVDTP transport; integrate with §18 mixer node.
- HFP: SLC + audio over SCO; AT command set.
- HID: Boot keyboard + report descriptor parser; events into §30 input.

### T7. Pairing manager — `userspace/willobtd/pair.rs`
- SMP (Security Manager Protocol) for BLE; legacy pairing for BR/EDR.
- Compositor prompt for Numeric Comparison / Passkey via D-Bus portal.

### T8. willobtctl + willobt-settings plugin — `userspace/willobtctl/`, `userspace/willoc-settings/plugins/bt.rs`
- CLI: scan, pair, connect, disconnect, info.
- Settings panel plugin lists devices, status, battery.

## New / modified files

| Path | Change |
| --- | --- |
| `kernel/src/bt/hci.rs` | **new** |
| `kernel/src/bt/l2cap.rs` | **new** |
| `kernel/src/bt/sco.rs` | **new** |
| `userspace/willobtd/` | **new** |
| `userspace/willobtctl/` | **new** |
| `userspace/willoc-settings/plugins/bt.rs` | **new** plugin |

## Tests to add

- `kernel/tests/hci_loopback.rs` — emulated HCI; round-trip command/event.
- `userspace/willobtd/tests/pair_numeric.rs` — Numeric Comparison pairing.
- `userspace/willobtd/tests/gatt_battery.rs` — read battery characteristic.
- `userspace/willobtd/tests/a2dp_play.rs` — A2DP sink path → §18 mixer.

## Risks & open questions

- **Pairing fall-back to weaker auth** — enforce IO capability negotiation; reject pairings that downgrade to Just Works on devices with display/keypad.
- **AAC patent** — default ship SBC; AAC behind opt-in codec pack (§40).
- **HFP wideband (mSBC)** — supported; TBD on more codecs (LC3 plus LE Audio in §43.x).
- **Driver coverage** — start with one BT-USB chipset (BCM-class or Intel-class); document the rest as TODO.
- **Battery** — peer device battery via 0x180F GATT service; expose in §36 system monitor.
- **Coexistence with Wi-Fi** — share antennas; mac80211 must signal BT for blanking.
