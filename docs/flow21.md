# flow21 — M21: architecture & runtime flows (Wi-Fi)

> Derived from `docs/idea.md` §8 and the work21 plan.

## Component map

```
        userspace
        +----------------------+        +------------------+
        |  willo-supplicant    |<------>| /etc/willo/wifi  |
        +----------+-----------+        +------------------+
                   |  nl80211-class socket
                   v
        +-------------------------------------------------+
        |   kernel: net/wifi/nl80211 + net/wifi/cfg80211  |
        +---------+---------------------------+-----------+
                  |                           |
                  v                           v
        +------------------+        +-----------------------+
        | mac80211 SoftMAC |<------>| key + rate ctrl hooks |
        +--------+---------+        +-----------------------+
                 |
                 v
        +-----------------+        +---------------------+
        |  driver layer   |<-----> |   crypto (CCMP/GCMP)|
        +--------+--------+        +---------------------+
                 |
                 v
              PCIe radio
                 |
                 v
              air (802.11)
```

## wlan0 registration at boot (deltas vs. M14)

1. PCI scan (M13) finds Wi-Fi vendor/device IDs → bind `qemuwlan` driver.
2. Driver constructs a `wiphy` and calls `cfg80211::register_wiphy()`.
3. `cfg80211` allocates `wlan0` net device and calls `netstack::add_iface()` (M14).
4. nl80211 control socket comes up; `willo-supplicant` connects.
5. Until `willo-supplicant` issues `ASSOCIATE`, the link stays operationally down (no DHCP traffic).

## Association flow (WPA2-PSK)

```
willo-supplicant                kernel                          driver / radio
    |                              |                                  |
    | TRIGGER_SCAN ----------------> mac80211::scan_request --------->|
    |                              |   (active probe on each ch)     |
    | <--- SCAN_RESULTS ------------|<--- frame::Beacon/ProbeResp ----|
    | AUTHENTICATE (open) --------->|                                 |
    |                              | tx_mgmt(Auth req) -------------->|
    | <--- AUTH_OK -----------------|<-- Auth resp -------------------|
    | ASSOCIATE ------------------->|                                 |
    |                              | tx_mgmt(AssocReq) -------------->|
    | <--- ASSOC_OK ----------------|<-- AssocResp -------------------|
    | EAPOL 4-way handshake (KCK/KEK/PTK derived in supplicant) <---->|
    | SET_KEY (PTK) --------------->| key install (CCMP)              |
    | NL80211_CMD_CONNECT ---------->| link up; netstack notified      |
```

## DHCP-over-wlan0 (data plane)

```
netstack::dhcp::send_discover()
  -> 802.3 frame (Eth + IP/UDP/BOOTP)
  -> wifi::netif::tx(skb)
       -> mac80211::tx
            (wraps in 802.11 QoS-data, encrypts CCMP)
       -> driver::tx_pkt
       -> radio
                                    ... AP responds OFFER ...
  <- driver::rx_pkt
       <- mac80211::rx
            (decrypt CCMP, A-MPDU reorder, dedupe)
       <- 802.3 to netstack
  netstack::dhcp::recv_offer()
```

## Disconnect / reconnect

```
explicit DISCONNECT (user) or beacon-loss timeout (mac80211 internal):
    mac80211 -> tx_mgmt(Deauth) -> driver -> radio
    cfg80211 -> netstack::link_down(wlan0)
    free PTK/GTK; reset MLME to Unjoined
    nl80211 emits CMD_DISCONNECT to supplicant
    supplicant retries with backoff (1s, 2s, 5s, 10s)
```

## Failure paths

- **Driver tx ENOMEM** → mac80211 drops skb, increments `tx_errors`; not surfaced as panic.
- **EAPOL timeout (3 s)** → supplicant sends Deauth, retries; surfaces to UI via §28 online accounts after 3 fails.
- **Wrong PSK** → 4WH fails at MIC verify; supplicant emits "auth failed" event; nothing is keyed in kernel.
- **Beacon loss > 10× listen interval** → mac80211 transitions to Unjoined, netstack marks `wlan0` down.
- **Regulatory deny on TX channel** → driver returns `EPERM`; mac80211 surfaces `OPER_NOT_PERMITTED`.

## Data structures

```rust
pub struct Wiphy {
    pub bands: [Band; 3],            // 2.4 / 5 / 6 GHz
    pub ciphers: u32,                // bitmask: CCMP, GCMP, ...
    pub max_scan_ssids: u8,
    pub regdom: [u8; 2],             // ISO-3166, "00" = world
}

pub enum MlmeState {
    Unjoined,
    Scanning { until: TickInstant },
    Authenticated { bssid: [u8; 6] },
    Associated { aid: u16 },
    Connected { ptk: KeyId, gtk: KeyId },
}

pub struct ScanResult {
    pub bssid: [u8; 6],
    pub ssid: heapless::String<32>,
    pub channel: u16,
    pub rssi_dbm: i8,
    pub cipher_suites: u32,
    pub akm_suites: u32,
}

pub trait WiFiDevice: Send + Sync {
    fn start_scan(&self, req: &ScanRequest) -> Result<(), Errno>;
    fn set_channel(&self, ch: u16) -> Result<(), Errno>;
    fn tx_mgmt(&self, frame: &[u8]) -> Result<(), Errno>;
    fn set_key(&self, k: &KeyConfig) -> Result<KeyId, Errno>;
}
```
