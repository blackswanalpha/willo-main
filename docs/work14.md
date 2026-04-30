# work14 — M14: virtio-net + TCP/IP + DHCP + DNS

> Derived from `docs/idea.md` §8 (Networking).

## Goal

Get Willo online. Bring up a virtio-net NIC, build (or port) a TCP/IP stack, lease an IP via DHCP, resolve names via DNS, and run a userspace `ping` + `httpget` to prove the round-trip works.

## Depends on

- **M13** — PCI + MSI-X (NIC discovery + IRQ delivery), slab allocator (skbuffs).

## Acceptance criteria

- [ ] `virtio-net` device discovered via PCI; queues set up.
- [ ] Layer 2: Ethernet frames sent + received.
- [ ] ARP / NDP resolves gateway.
- [ ] IPv4: full datagram path; IPv6 stub (RA accept + link-local only).
- [ ] ICMP echo reply.
- [ ] UDP socket API works end-to-end (DHCP uses it).
- [ ] TCP socket API: 3-way handshake, ordered delivery, retransmit, FIN close. Loss recovery sufficient for HTTP/1.1.
- [ ] DHCP client gets a lease from QEMU's user-mode networking.
- [ ] DNS resolver (UDP + TCP fallback) returns A records.
- [ ] `userspace/ping/` and `userspace/httpget/` work against `example.com`.

## Task breakdown

### T1. virtio-net driver — `kernel/src/drivers/virtio/net.rs` (new)
- Reuse a small virtio-core (`virtio/queue.rs`).
- Set up RX + TX virtqueues, feature negotiation.
- IRQ handler hands buffers to the netstack.

### T2. Network stack core — `kernel/src/net/mod.rs` (new)
- Decision: write a minimal stack in-tree rather than port `smoltcp` — ABI control matters for §9 socket compatibility. Reconsider mid-milestone if scope creeps.
- Modules: `eth.rs`, `arp.rs`, `ipv4.rs`, `ipv6.rs`, `icmp.rs`, `udp.rs`, `tcp.rs`.
- `Skb` (socket buffer) backed by slab.

### T3. Routing + neighbour — `kernel/src/net/route.rs`, `kernel/src/net/neigh.rs`
- One default route initially; `/etc/resolv.conf`-equivalent driven by DHCP.
- ARP cache with timeouts.

### T4. Socket layer — `kernel/src/net/socket.rs`
- `socket()`, `bind()`, `listen()`, `accept()`, `connect()`, `send()`, `recv()`, `close()`.
- POSIX-ish errnos.
- Wired into the syscall table from M11.

### T5. DHCP client — `userspace/dhcpc/` (new)
- DISCOVER → OFFER → REQUEST → ACK; renew at T1.
- Writes lease + DNS into `/etc/dhcp/lease.json` (FAT for now; sysfs in M16).

### T6. DNS resolver — `kernel/src/net/dns.rs` + libc helper
- UDP query, fall back to TCP on truncation.
- Tiny in-kernel cache (32 entries) for NSS-style libc lookup.
- Long term, this should move to userspace; cheap stub now.

### T7. Userspace tools — `userspace/ping/`, `userspace/httpget/`
- `ping` — raw ICMP, requires a CAP_NET_RAW analogue or bypass for now (will tighten in §12).
- `httpget url` — GET, print body to stdout.

## New / modified files

| Path | Change |
| --- | --- |
| `kernel/src/drivers/virtio/queue.rs` | **new** (shared virtio core) |
| `kernel/src/drivers/virtio/net.rs` | **new** |
| `kernel/src/net/{mod,eth,arp,ipv4,ipv6,icmp,udp,tcp,route,neigh,socket,dns}.rs` | **new** |
| `kernel/src/syscall.rs` | + socket syscalls |
| `userspace/dhcpc/`, `userspace/ping/`, `userspace/httpget/` | **new** |
| `src/main.rs` (runner) | `-netdev user,id=u1 -device virtio-net-pci,netdev=u1` |

## Tests to add

- `kernel/tests/net_arp.rs` — ARP lookup against a fake NIC backend.
- `kernel/tests/net_udp_loop.rs` — UDP echo via host with QEMU port-forward.
- `kernel/tests/net_tcp_handshake.rs` — `connect` to QEMU host, exchange bytes.
- `kernel/tests/dhcp_lease.rs` — lease acquired against QEMU user-mode net.
- `kernel/tests/dns_resolve.rs` — resolve `example.com`.

## Risks & open questions

- **In-tree stack vs. `smoltcp`** — if the stack ends up >5 kLOC, fall back to `smoltcp`.
- **TCP corner cases** — keep window scaling, SACK off for v1; document gaps.
- **Endianness mistakes** — adopt `ByteOrder` types throughout; no manual swaps.
- **Privilege for raw sockets** — use a coarse "root only" check until §12 capabilities land.
