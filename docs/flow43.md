# flow43 — M43: architecture & runtime flows (Bluetooth)

> Derived from `docs/idea.md` §8 and the work43 plan.

## Component map

```
        userspace
        +-------------+   +-------------------------+
        | willobtctl  |   | settings plugin (BT)    |
        +------+------+   +-----------+-------------+
               |                       |
               v                       v
        +-----------------------------------+
        |             willobtd               |
        | gatt | sdp | pair | profiles      |
        |  a2dp | hfp | hid                  |
        +--+----+-----+--+----+-+-----+------+
           |     |        |     |     |
           v     v        v     v     v
        ATT/  SDP/   AVDTP   AT    HID
        SMP   L2CAP   L2CAP  cmds  rep
           \    |       |     |    /
            \   |       |     |   /
             v  v       v     v  v
        +---------------------------+
        |   kernel: l2cap | sco     |
        +-------------+-------------+
                      |
                      v
                 +---------+
                 |   HCI   |
                 +----+----+
                      |
                      v
                 USB BT controller
```

## Scan / advertise

```
willobtctl scan on
  willobtd -> HCI LE Set Scan Parameters + Set Scan Enable
    advertisements arrive as HCI events
    parsed: addr_type, addr, RSSI, AD records (name, services, MFG data)
  willobtd D-Bus signal: DeviceFound { addr, name, rssi, uuids }
willobtctl pair AA:BB:CC:DD:EE:FF
```

## Pairing (Numeric Comparison, BLE)

```
SMP Pairing Request -> Pairing Response (capabilities exchange)
  IO caps: DisplayYesNo on both -> Numeric Comparison
ECDH key exchange (P-256)
DHKey check
both sides display 6-digit value
willobtd -> compositor portal:
  show "Pair with $name? Code: 482931 [Confirm] [Cancel]"
user confirms -> SMP Confirm; LTK derived; encrypted; persisted to /var/lib/willobt/keys/
```

## A2DP sink (phone -> Willo)

```
phone connects (it's the source)
SDP query: A2DP Sink UUID supported
AVDTP Discover/GetCapabilities/SetConfiguration (codec=SBC, params)
Open + Start -> AVDTP media stream over L2CAP
willobtd::profiles::a2dp:
  decode SBC frames (or pass through to §18 if HW codec)
  push PCM to §18 audio core mixer node ("BT phone")
user plays music -> heard from Willo speakers
```

## HID (BT keyboard)

```
keyboard advertises (Just Works pairing if no display); pair
on connect:
  SDP -> HID descriptor (boot or report)
  L2CAP HID Control + Interrupt channels open
  willobtd::profiles::hid:
     parse report descriptor
     translate report -> Linux-input-class evdev events
     route to §30 input subsystem
typing -> focused app receives keys
```

## GATT client (battery service)

```
willobtctl info AA:..  showing battery
  willobtd:
    L2CAP open ATT (PSM 0x0004)
    Discover Primary Services -> 0x180F (Battery Service)
    Discover Characteristics -> 0x2A19 (Battery Level)
    Read Characteristic Value -> u8 0..100
    optional Subscribe Notifications
  return to UI: 73%
```

## Failure paths

- **Pairing canceled** → SMP Pairing Failed; willobtd cleans state; no key stored.
- **Lost link** → HCI Disconnection Complete; profile state torn down; UI shows "disconnected"; auto-reconnect on advertisement if bonded.
- **Codec negotiation fail** (A2DP) → fall back to SBC mandatory; warn if user prefers AAC.
- **HFP audio glitch** → mSBC packets dropped; §18 mixer fills with silence frames; counter for telemetry.
- **HCI command timeout** → driver resets controller; willobtd reattaches.

## Data structures

```rust
pub struct HciDevice {
    pub id: u8,
    pub bdaddr: BdAddr,                     // 6 bytes
    pub features: HciFeatures,
    pub state: HciState,                    // Up | Down | Resetting
}

pub struct L2capChannel {
    pub psm: u16,
    pub cid: u16,
    pub mode: L2capMode,                    // Basic | ERTM | Streaming
    pub mtu_local: u16,
    pub mtu_remote: u16,
}

pub struct GattAttribute {
    pub handle: u16,
    pub uuid: Uuid,                         // 16 or 128 bit
    pub value: Vec<u8>,
    pub perms: AttPerms,                    // read/write/notify/indicate
}

pub struct PairingState {
    pub peer: BdAddr,
    pub method: PairingMethod,              // JustWorks | NumericComparison | Passkey | Oob
    pub io_caps: IoCaps,
    pub ltk: Option<[u8; 16]>,
    pub bonded: bool,
}
```
