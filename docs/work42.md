# work42 — M42: WireGuard kernel module + VPN hooks (TUN/TAP)

> Derived from `docs/idea.md` §8 (Networking — VPN hooks, TUN/TAP, WireGuard).

## Goal

Land in-kernel **WireGuard** with the standard Noise_IKpsk2_25519_ChaChaPoly_BLAKE2s handshake, plus a generic **TUN/TAP** virtual NIC and an `iptables`/`nftables`-class **firewall** hook surface used by both VPN and §41 containers. Userland: `willo-wg` (`wg`-class CLI) and `willowg-quick` (interface bring-up).

## Depends on

- **M14** — netstack + UDP + sockets.
- **M21/M41** — alternate ifaces + namespaces.
- **M39** — VPN keys live in keyring (via §38).

## Acceptance criteria

- [ ] `/dev/net/tun` exists; opening + ioctl `TUNSETIFF` creates `tunN`/`tapN`.
- [ ] `wg setconf wg0 wg0.conf` sets private key, peers, allowed-ips, endpoints.
- [ ] WireGuard handshake completes against an upstream peer in QEMU; data plane carries ICMP/TCP.
- [ ] Per-peer counters (`rx_bytes`, `tx_bytes`, `last_handshake_time`) exposed via `wgctrl`-class API.
- [ ] `persistent_keepalive` keeps NAT pinned through 2-min idle.
- [ ] Endpoint roaming: peer source IP changes mid-session; subsequent packets use new endpoint.
- [ ] `kernel/tests/wg_handshake.rs` — handshake against a libwireguard test server.
- [ ] `kernel/tests/tun_tap_pingpong.rs` — TUN userland app round-trips packets.
- [ ] `kernel/tests/firewall_basic.rs` — drop rule applied; ping fails.

## Task breakdown

### T1. TUN/TAP — `kernel/src/net/tun.rs`
- `/dev/net/tun` char dev; `TUNSETIFF` (TUN/TAP/MULTI_QUEUE), `TUNSETPERSIST`, `TUNSETOWNER`.
- skb tx/rx between userland fd and netstack.

### T2. WireGuard core — `kernel/src/net/wg/`
- Noise_IK handshake state machine (initiator + responder); cookies for DoS resistance.
- Crypto: x25519, chacha20poly1305, blake2s; constant-time impls.
- Per-peer tx/rx counters + replay window (sliding-bitmap, length 8192).

### T3. WireGuard packet codec — `kernel/src/net/wg/packet.rs`
- Wire format: type(1) sender(4) counter(8) ciphertext+tag.
- Cookie reply, handshake init/response, transport data.

### T4. wgctrl-class API — `kernel/src/net/wg/genl.rs`
- Netlink-class control plane shaped like Linux WG_CMD_GET_DEVICE / WG_CMD_SET_DEVICE.

### T5. Firewall hook surface — `kernel/src/net/fw.rs`
- Hooks at PRE_ROUTING, POST_ROUTING, INPUT, OUTPUT, FORWARD.
- Rule = (match, action). Match: src/dst CIDR, protocol, port, iif/oif. Action: accept/drop/reject/log/jump.
- nftables-class table + chain + rule ABI; first-class `set`s.

### T6. willo-wg + willowg-quick — `userspace/willo-wg/`, `userspace/willowg-quick/`
- `willo-wg` matches Linux `wg` CLI surface.
- `willowg-quick` parses `.conf`, brings up interface, applies AllowedIPs routing + masquerade.

### T7. Integration with §28 online accounts — `userspace/willo-wg/accounts.rs`
- Optional: store private + preshared keys via §38 SecretService.

### T8. Container netns helper — `userspace/willo-crun/wg.rs`
- Move a wg interface into a container netns at start; restore on exit.

## New / modified files

| Path | Change |
| --- | --- |
| `kernel/src/net/tun.rs` | **new** |
| `kernel/src/net/wg/*` | **new** module |
| `kernel/src/net/fw.rs` | **new** |
| `kernel/src/syscall.rs` | + `setsockopt(SO_ATTACH_FILTER)` parity |
| `userspace/willo-wg/` | **new** |
| `userspace/willowg-quick/` | **new** |

## Tests to add

- `kernel/tests/tun_tap_pingpong.rs` — userland write to tun, kernel routes back.
- `kernel/tests/wg_handshake.rs` — handshake + first encrypted packet.
- `kernel/tests/wg_roaming.rs` — endpoint moves; peer recovers.
- `kernel/tests/firewall_basic.rs` — drop rule blocks ping.
- `userspace/willo-wg/tests/conf_roundtrip.rs` — parse + emit identical `.conf`.

## Risks & open questions

- **Crypto correctness** — use audited Rust impls (x25519-dalek-class, chacha20poly1305 RustCrypto); add fuzz harness against test vectors from RFCs.
- **Replay window edge cases** — counter wrap is theoretical (2^64); document.
- **NAT timeout** — `persistent_keepalive` default 25 s; expose toggle.
- **Performance** — kernel WG with AVX2 ChaCha matches Linux; without AVX2 expect ~30% slower.
- **Container integration** — moving WG into netns must respect refcount; tested in §41.
- **IPv6** — first-class; allow ULA + GUA both.
