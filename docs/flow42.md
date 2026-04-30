# flow42 — M42: architecture & runtime flows (WireGuard + TUN/TAP + firewall)

> Derived from `docs/idea.md` §8 and the work42 plan.

## Component map

```
        userspace
        +-----------------+   +---------------------+
        | willo-wg / -quick|   | willowg-quick conf  |
        +--------+--------+   +----------+----------+
                 |  wgctrl genl-class    |
                 v                       v
        +-----------------------------------------+
        |   kernel net/wg                          |
        |   handshake | xfrm | counters | replay   |
        +-----------+-------------------+----------+
                    |                   |
                    v                   v
                  netstack          UDP socket (53/51820)
                    |                   |
                    v                   v
                  TUN/TAP             NIC / wlan0
```

## TUN/TAP open

```
fd = open("/dev/net/tun", O_RDWR)
ioctl(fd, TUNSETIFF, &ifr{name="tun0", flags=IFF_TUN|IFF_NO_PI})
  -> kernel allocates tun0 net_device; userland fd is its data path
ip addr add 10.0.0.1/24 dev tun0
ip link set tun0 up
userland: read(fd) -> packets going out via tun0 (no L2 header)
userland: write(fd, pkt) -> packets injected as if received by tun0
```

## WireGuard interface bring-up

```
willowg-quick up wg0
  -> read /etc/willo/wg/wg0.conf
  -> ip link add dev wg0 type wireguard
  -> wg setconf wg0 wg0.conf:
       PrivateKey = ... (read from file or §38)
       ListenPort = 51820
       Peer { PublicKey, AllowedIPs, Endpoint, PersistentKeepalive }
  -> ip address add ...
  -> ip route add <AllowedIPs> dev wg0
  -> nftables-class: NAT/masquerade if Address has 0.0.0.0/0
```

## Handshake (Noise_IKpsk2)

```
initiator (us) -> responder (peer):
  message_type=1 (init)
    sender_index = random
    ephemeral = E_pub
    static = S_pub_encrypted_with_HKDF_chain
    timestamp = TAI64N encrypted
    mac1 = mac(static_resp_pub, packet[..mac1])
    mac2 = mac(cookie, packet[..mac2])  // 0 if no cookie

responder -> initiator:
  message_type=2 (response)
    sender_index, receiver_index
    ephemeral, empty_payload encrypted
    derive transport keys via HKDF
both sides now have send_key + recv_key + counters
```

## Transport packet path

```
app sends to 10.0.0.42 (peer)
  -> netstack route: dev wg0
  -> wg encap:
       counter += 1
       ciphertext = chacha20poly1305(key, counter, plaintext)
       tag appended; UDP send to peer endpoint
peer receives:
  -> wg decap:
       check counter against replay window (8192 wide)
       decrypt+verify; if OK, deliver to inner netstack
```

## Endpoint roaming

```
peer's NAT changes (laptop walked Wi-Fi → cellular):
  packet arrives from new src IP/port
  wg::on_recv:
    - validate decrypt OK
    - update peer.endpoint = new src
  outbound packets henceforth use the new endpoint
```

## Firewall + NAT

```
nft add table inet willo
nft add chain inet willo forward { type filter hook forward priority 0; }
nft add rule inet willo forward iifname wg0 ip saddr 10.0.0.0/24 ct state established,related accept
nft add rule inet willo forward iifname wg0 drop
nft add chain inet willo postrouting { type nat hook postrouting priority 100; }
nft add rule inet willo postrouting oifname wlan0 masquerade
```

## Containers + WireGuard (linkage with §41)

```
willo-crun start --net wg0
  -> ip link set wg0 netns <pid>
  -> inside container: wg0 reachable; outside no longer sees it
on container exit:
  -> netns destroyed; wg0 returns to host (via `ip link set wg0 netns 1`) OR is destroyed
```

## Failure paths

- **Wrong public key** → handshake init MAC1 mismatch; silently dropped (anti-DoS).
- **Counter replay** → outside replay window → drop + counter increment.
- **NAT timeout** → keepalive packet sent automatically per peer.persistent_keepalive.
- **Firewall rule conflict** → first-match wins; ordering documented; surfaced via `willo-wg show`.
- **TUN fd close while interface up** → kernel marks interface "no carrier"; netstack drops packets; restores when fd reopens.

## Data structures

```rust
pub struct WgPeer {
    pub public_key: [u8; 32],
    pub preshared_key: Option<[u8; 32]>,
    pub endpoint: Option<SocketAddr>,
    pub allowed_ips: Vec<IpNet>,
    pub persistent_keepalive: Option<Duration>,
    pub last_handshake_time: Option<TickInstant>,
    pub rx_bytes: AtomicU64,
    pub tx_bytes: AtomicU64,
    pub session: Option<WgSession>,           // active transport keys + counters
}

pub struct WgSession {
    pub send_key: [u8; 32],
    pub recv_key: [u8; 32],
    pub send_counter: AtomicU64,
    pub recv_window: ReplayWindow,           // 8192-bit sliding bitmap
    pub created_at: TickInstant,
}

pub struct FwRule {
    pub table: SmolStr,
    pub chain: ChainHook,                    // PreRouting | PostRouting | Input | Output | Forward
    pub matches: Vec<Match>,
    pub action: Action,                      // Accept | Drop | Reject | Log | Jump(SmolStr)
}

pub struct TunDev {
    pub name: SmolStr,
    pub flags: TunFlags,                     // TUN | TAP | NO_PI | MULTI_QUEUE
    pub fd_holders: Vec<FileHandle>,
}
```
