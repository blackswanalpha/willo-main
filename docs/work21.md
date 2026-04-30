# work21 — M21: Wi-Fi (cfg80211/mac80211-class core, one PCIe driver, willo-supplicant)

> Derived from `docs/idea.md` §8 (Networking).

## Goal

Bring 802.11 connectivity to Willo. Land a Linux-shaped split between a kernel **cfg80211-class** registration/config layer and a **mac80211-class** SoftMAC core, drive one PCIe Wi-Fi card behind it, and ship a userspace `willo-supplicant` that performs WPA2/WPA3-PSK authentication. The result is a `wlan0` interface plugged into the M14 netstack so DHCP + DNS + `curl` "just work" over Wi-Fi.

## Depends on

- **M14** — TCP/IP stack + DHCP + DNS (for the data plane).
- **M13** — PCI enumeration + APIC/MSI (for binding the radio).
- **M11/M12** — userspace + IPC (for `willo-supplicant`).

## Acceptance criteria

- [ ] `wlan0` enumerates from PCI on boot and appears in `ip link`-equivalent output.
- [ ] `willo-supplicant -c wifi.conf` associates with a hidden + a broadcast SSID using WPA2-PSK in QEMU's `hostapd`.
- [ ] WPA3-SAE handshake completes against the same `hostapd` with `sae` enabled.
- [ ] Active scan returns ≥1 entry per visible SSID with RSSI + channel + cipher suite.
- [ ] DHCP completes over `wlan0`; `curl https://example.com` succeeds.
- [ ] `kernel/tests/wifi_assoc.rs` runs the full scan → auth → assoc → 4-way handshake → first DHCP packet path.
- [ ] Disconnect + reconnect within 30 s does not leak skbs (heap delta ≤ 4 KiB).

## Task breakdown

### T1. 802.11 frame parser — `kernel/src/net/wifi/frame.rs` (new)
- Parse FC (frame-control), addr1/2/3/4, sequence, QoS, HT/VHT/HE caps IEs.
- Encode probe-request, auth, assoc-request, deauth, EAPOL-Key.
- Round-trip fuzz harness in `kernel/tests/wifi_frame_fuzz.rs`.

### T2. mac80211-class SoftMAC core — `kernel/src/net/wifi/mac80211/`
- MLME state machine: `Unjoined → Scanning → Authenticated → Associated → 4WH → Connected`.
- Rx/Tx pipelines: 802.11 ↔ 802.3 framing, A-MPDU re-order buffer, sequence-number deduplication.
- Key management hooks (CCMP/GCMP) — actual crypto in `kernel/src/crypto/`.
- Rate-control hook (start with fixed lowest-rate; adaptive later).

### T3. cfg80211-class registration — `kernel/src/net/wifi/cfg80211.rs` (new)
- `trait WiFiDevice` (driver-facing): `start_scan`, `set_channel`, `tx_mgmt`, `set_key`.
- Per-device wiphy struct: supported bands, ciphers, regulatory class.
- Userspace control plane via a Willo-native netlink-class socket.

### T4. nl80211-class control socket — `kernel/src/net/wifi/nl80211.rs`
- Family registration with command/attribute IDs aligned to Linux nl80211 numbers (so a future Linux-compat shim is cheap).
- Commands: `GET_WIPHY`, `TRIGGER_SCAN`, `GET_SCAN`, `AUTHENTICATE`, `ASSOCIATE`, `DISCONNECT`, `SET_KEY`.

### T5. PCIe driver — `kernel/src/net/wifi/drivers/qemuwlan.rs` (new)
- Targets QEMU's `hwsim`/virtio-style WLAN PHY for CI; behind a feature flag.
- Stretch: stub for one real card (e.g. Intel iwlwifi-class) gated `--features iwl`.

### T6. userspace `willo-supplicant` — `userspace/willo-supplicant/`
- Speaks nl80211-class to kernel.
- WPA2-PSK + WPA3-SAE state machines (port `wpa_supplicant` SAE, or write minimal); EAPOL 4-way handshake.
- Config file `/etc/willo/wifi.conf` (TOML).

### T7. wlan0 ↔ netstack glue — `kernel/src/net/wifi/netif.rs`
- Register `wlan0` with M14 net device list; route 802.3 frames to/from netstack.
- Power-saving hooks (PS-Poll / U-APSD) deferred to M23.

### T8. Regulatory + channel mgmt — `kernel/src/net/wifi/regdom.rs`
- Hard-code a permissive `00` (world) regdom for v1; CRDA-class fetch deferred.

## New / modified files

| Path | Change |
| --- | --- |
| `kernel/src/net/wifi/frame.rs` | **new** |
| `kernel/src/net/wifi/mac80211/*` | **new** |
| `kernel/src/net/wifi/cfg80211.rs` | **new** |
| `kernel/src/net/wifi/nl80211.rs` | **new** |
| `kernel/src/net/wifi/drivers/qemuwlan.rs` | **new** |
| `kernel/src/net/wifi/netif.rs` | **new** |
| `kernel/src/net/wifi/regdom.rs` | **new** |
| `kernel/src/net/mod.rs` | wire in `wifi::init()` |
| `userspace/willo-supplicant/` | **new** |
| `Cargo.toml` (root) | add userspace member |

## Tests to add

- `kernel/tests/wifi_frame_fuzz.rs` — round-trip + length-bound fuzz.
- `kernel/tests/wifi_assoc.rs` — scan → 4WH → DHCP first packet against `hostapd`.
- `kernel/tests/wifi_disconnect.rs` — assoc, disconnect, reconnect; assert no skb leak.
- `userspace/willo-supplicant/tests/sae.rs` — WPA3-SAE vectors from RFC 7664.

## Risks & open questions

- **FullMAC vs SoftMAC** — committed to SoftMAC for v1; FullMAC firmware bugs are undebuggable on a hobby OS.
- **Crypto ABI** — CCMP/GCMP rely on M13's slab + a future AES-NI path; until then, pure-Rust AES (slow but correct).
- **Regulatory** — shipping `00` is fine for QEMU but real radios will refuse high-power channels; surface a TODO + a CRDA-class plan for M30+.
- **Power save** — explicit non-goal in M21; revisit alongside §14 ACPI in M23.
- **Driver coverage** — only QEMU `hwsim` lands by default; one real driver is best-effort behind a feature flag.
