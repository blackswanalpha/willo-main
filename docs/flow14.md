# flow14 — M14: architecture & runtime flows

> Derived from `docs/idea.md` §8 and the work14 plan.

## Component map

```
   userspace                          kernel
+-------------+   syscall(socket)   +---------------------+
| httpget     |-------------------->|  socket layer       |
| ping        |                     +----+----+-----+-----+
| dhcpc       |                          |    |     |
+-------------+                          v    v     v
                                       udp  tcp   icmp
                                          \  |   /
                                           v v  v
                                         +------+
                                         | ipv4 |  (+ ipv6 stub)
                                         +--+---+
                                            |
                                       +----+-----+
                                       |   eth    |  (+ arp/neigh)
                                       +----+-----+
                                            |
                                            v
                                  +------------------+
                                  |  virtio-net      |  (PCI + MSI-X)
                                  +--------+---------+
                                           |
                                          NIC
```

## Skb path

```
TX:
  socket.send(buf)
   └─ skb = Skb::new()
      ├─ tcp/udp fills L4
      ├─ ipv4 prepends L3, picks route
      ├─ neigh resolves dst_mac (ARP if needed)
      ├─ eth prepends L2
      └─ virtio_net.tx_enqueue(skb)
         └─ NIC IRQ on completion → skb dropped

RX:
  NIC IRQ
   └─ virtio_net.rx_dequeue() -> skb
      ├─ eth strips L2, dispatches by ethertype
      ├─ ipv4 strips L3, dispatches by protocol
      ├─ tcp/udp/icmp handles
      └─ socket wq woken
```

## DHCP sequence

```
boot → dhcpc starts
  ├─ socket(UDP, 0.0.0.0:68)
  ├─ DISCOVER (broadcast)
  ├─ recv OFFER  → server, yiaddr
  ├─ REQUEST yiaddr from server
  ├─ recv ACK    → ip, mask, gw, dns, lease
  └─ write /etc/dhcp/lease.json
        ipv4::set_addr/route/dns(...)
```

## TCP state machine (subset)

```
   CLOSED ── connect ──> SYN_SENT ── SYN+ACK ──> ESTABLISHED
                                                    │
                                                    │ close
                                                    v
                                                FIN_WAIT_1
                                                    │ ACK
                                                    v
                                                FIN_WAIT_2
                                                    │ FIN
                                                    v
                                                  CLOSED
```

Listen / accept / passive close paths included; keep-alive deferred.

## DNS lookup

```
gethostbyname("example.com")
 ├─ cache.get → hit? return
 ├─ udp.send(53, A query)
 ├─ wait reply (timeout 5s)
 ├─ if TC=1: tcp.send(53, same query)
 ├─ parse answers
 └─ cache.put(name, ips, ttl)
```

## Socket → syscall mapping

| `rax` | name      |
| ----- | --------- |
| 41    | `socket`  |
| 42    | `connect` |
| 43    | `accept`  |
| 44    | `sendto`  |
| 45    | `recvfrom`|
| 49    | `bind`    |
| 50    | `listen`  |
| 51    | `getsockname` |
| 52    | `getpeername` |

## Memory & buffers

- `Skb` allocated from slab; uses scatter-gather to avoid a copy on enqueue.
- TX queue depth: 256; RX: 256.
- ARP cache: 64 entries, 5-minute TTL.

## Failure paths

- NIC stalled (no completions in 5 s) → driver resets device, drops in-flight skbs.
- Routing miss → `ENETUNREACH`.
- ARP miss after 3 retries → `EHOSTUNREACH`.
- DHCP timeout → exponential backoff; netstack stays unconfigured.
- DNS NXDOMAIN → returns empty list, `EAI_NONAME` to libc.
