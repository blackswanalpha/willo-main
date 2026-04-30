# flow58 — cross-cutting: network packet end-to-end (NIC / wlan / wg → app socket)

> Cross-cutting flow citing M14, M21, M41, M42.

How a packet travels from the wire to a userspace `recv`, and back. Useful for debugging "why doesn't curl work?" and for benchmarking.

## Receive path (TCP from wire to app)

```
wire (Ethernet/802.11/WG-UDP)
  │
  v
NIC / radio / WG endpoint
  │   IRQ (or threaded IRQ in §44)
  v
driver rx ringbuffer
  │   skb allocated; copy or DMA-mapped
  v
netif_receive_skb (M14)
  │
  v
interface-specific hooks:
  M21 wlan0:
     mac80211 decrypt CCMP/GCMP
     802.11 → 802.3 frame conversion
  M42 wg0:
     wg::decrypt_packet (counter check + chacha20poly1305)
     replay window update
  none (eth0): direct
  │
  v
ip_rcv (IPv4) or ipv6_rcv (IPv6)
  │
  v
firewall PRE_ROUTING hooks (§42 fw)
  match rules; accept/drop/log
  │
  v
routing:
  local? -> ip_local_deliver
  forward? -> ip_forward (containers via §41 namespaces)
  │
  v
ip_local_deliver -> protocol handler:
  TCP -> tcp_v4_rcv (sequence/ack/cong)
     enqueue to socket recv queue
     wake task waiting on poll/recv
  UDP -> udp_rcv
     enqueue datagram
  ICMP -> icmp_rcv
  │
  v
socket recv queue
  │
  v
app: recv(fd, buf, len) returns bytes
```

## Send path (app to wire)

```
app: send(fd, buf, len)
  │
  v
socket layer
  │   TCP: segment + retransmit queue
  v
ip_local_out
  │
  v
firewall OUTPUT hooks
  │
  v
routing decision -> output dev
  M14: eth0 fast path
  M21: wlan0 mac80211 → driver tx → radio
  M42: wg0 → wg::encrypt → udp_send to peer endpoint -> outer NIC
  M41 container: dev belongs to container netns; iptables/nft NAT may rewrite
  │
  v
driver tx ring -> hardware -> wire
```

## Container netns example

```
container has lo + eth0 (veth0, peer veth1 on host bridge wbr0)
container app -> sendto -> netstack in container netns -> veth0
  -> kernel moves skb across veth pair to veth1 (host netns)
  -> bridge wbr0 forwards to wlan0 or wg0 per host route
return path symmetric (NAT rewrites if --net=bridge)
```

## WireGuard tunnel example

```
host has wg0 with peer 198.51.100.1:51820, AllowedIPs=10.0.0.0/24
app on host pings 10.0.0.5:
  -> route table: 10.0.0.0/24 dev wg0
  -> wg::encap:
        derive transport keys (already established by handshake)
        plaintext: ICMP echo request
        ciphertext = chacha20poly1305(send_key, counter, plaintext)
        wrap in UDP to 198.51.100.1:51820
  -> outer route: dev wlan0 -> radio -> internet
peer at 198.51.100.1:
  -> wg0 receives; decrypt; deliver to peer's netstack
  -> reply ping
inbound: outer NIC -> UDP -> wg::decap -> ICMP echo reply -> app
```

## Sockets API surface (M11/M14)

```
fd = socket(AF_INET, SOCK_STREAM, 0)
bind(fd, sockaddr_in { 0.0.0.0:0 })
connect(fd, sockaddr_in { 1.2.3.4:443 })
send(fd, buf, len)
recv(fd, buf, len)
close(fd)
```

## Observability

```
ss -tn (M14 socket dump)
  show ESTABLISHED + counters
ip -s link
  per-interface tx/rx + errors + drops
journalctl -t net
  link state changes; DHCP events; wg handshakes
willotrace (§29) bpf programs
  e.g. attach kprobe vfs_read on tcp_recvmsg
willomonitor (§36)
  net graph from /proc/net/dev at 1 Hz
```

## Latency budget (rough, idle laptop, no congestion)

```
NIC IRQ -> netif_receive_skb         ≈ 5-15 µs
ip_rcv -> tcp_v4_rcv                 ≈ 1-3 µs
firewall hooks (small ruleset)       ≈ 0.5-1 µs/hook
WG encrypt (1500 B, AVX2 ChaCha)     ≈ 2-3 µs
mac80211 encap + driver tx           ≈ 10-30 µs
total RX→app wakeup                  ≈ 50-100 µs typical
```

## Failure paths

- **Driver rx ring full** → kernel drops packets; counter `rx_dropped` increments; logged.
- **WG counter outside replay window** → drop + counter; not surfaced unless rate exceeds threshold.
- **Firewall drop** → counter on rule; optional `LOG` to journal.
- **No route** → `ENETUNREACH` to app.
- **Container netns deleted while pkt in flight** → kernel drops; container cleanup is atomic.
- **OOM during skb alloc** → `ENOMEM`; retried; if persistent, OOM killer engages.

## Tunables

- Per-iface `txqueuelen`.
- `net.core.somaxconn` (TCP listen backlog).
- WG `persistent_keepalive` per peer.
- Firewall logging rate-limit.
- BPF/eBPF program attach for custom telemetry (§29).

## Common debugging recipes

```
"curl https://example.com hangs" walk-through:
  1. ping example.com -> DNS works?
     DNS resolved? if no, check /etc/resolv.conf via M14 resolver
  2. ip route get <ip> -> route exists?
  3. curl -v https://example.com -> TLS handshake reach? if yes, network OK
  4. ss -tn -> connection state
  5. journalctl -t net -> link / DHCP / WG events
  6. willotrace tcp_v4_connect -> kprobe to see syscall outcome
```

## What about IPv6?

- First-class throughout (M14).
- `wlan0`, `eth0`, `wg0` all dual-stack.
- DHCPv6 + SLAAC supported.
- No-IPv4-fallback intentionally kept; warn if address-only.
