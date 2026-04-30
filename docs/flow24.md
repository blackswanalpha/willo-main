# flow24 — M24: architecture & runtime flows (Browser port)

> Derived from `docs/idea.md` §10, §21 and the work24 plan.

## Component map

```
        +----------------------------------------------+
        |                willo-browser                  |
        |  (Ladybird core + LibWeb + LibJS + LibGfx)    |
        +----+------------+-----------+-----------+-----+
             |            |           |           |
             v            v           v           v
        +--------+   +---------+ +---------+ +-----------+
        |  Qt    |   |  Mesa   | | rustls  | | willova   |
        |willo   |   |  Venus  | | + curl  | | codec     |
        +--+-----+   +----+----+ +----+----+ +-----+-----+
           |              |           |             |
           v              v           v             v
        compositor     kernel       netstack    audio core
        (M17)           DRM (M22)    (M14)       (M18)
                          |
                          v
                  libc-shim + libstdcxx-shim
                  (built atop M11/M12 syscalls)
```

## Browser launch sequence

```
willoshell$ willo-browser https://example.com
  -> §11 ELF loader maps Ladybird + Qt + Mesa + libcurl
  -> §39 MAC profile applied (renderer + main)
  -> Qt platform plugin selects "wayland"
  -> wl_registry::bind(linux-dmabuf-v1, xdg-shell, wp_viewporter)
  -> main process forks renderer (per-tab); each renderer reapplies §39 profile
  -> renderer loads page (see render flow)
```

## Page render flow

```
URL ── Ladybird::ResourceLoader
  -> rustls TLS handshake over §14 sockets
  -> HTTP/2 GET
  <- HTML, CSS, JS, images, fonts
  -> LibWeb HTML parser → DOM tree
  -> LibWeb CSS parser → CSSOM
  -> LibWeb layout
  -> LibJS engine (interpreter + JIT)
       -> mprotect RW → RX (audited via §39)
  -> LibGfx paint
       -> Skia
       -> Mesa Vulkan (Venus) submit
       -> §22 GEM/dma-buf
       -> compositor present (zero-copy)
```

## Network flow

```
Ladybird::HttpRequest
  -> libcurl
       -> rustls (TLS 1.3)
       -> §14 socket connect / send / recv
       -> netstack -> NIC / wlan0 (M21)
  <- bytes
  -> libcurl
  -> Ladybird::HttpResponse
```

## Media flow (HTML5 `<video>`)

```
<video src="movie.mp4">
  -> Ladybird MediaElement
  -> demux (mp4)
  -> §40 willova::decode (HW path via virtio-gpu / Intel)
       -> dma-buf decoded frames
  -> Skia composited with page
  -> compositor present
audio:
  -> §40 demux
  -> §18 audio core mixer
  -> HDA driver -> speakers
```

## JIT W^X flow

```
LibJS::CodeGenerator emits machine code into RW page
  -> mprotect(addr, RX, JIT_FLAG)
       -> kernel checks process capability "jit"
       -> §39 audit log: "pid X transitioned page Y RW->RX"
       -> page table flip; TLB shootdown
  -> indirect call into JITed code
```

## Sandbox boundaries

```
+---------------------------------------------+
| main process (browser chrome, network, UI)  |
|   §39 profile: net, file:/var/cache/browser |
|   read /etc/hosts, /etc/resolv.conf         |
+----------+----------------------------------+
           | shared mem + pipes
           v
+---------------------------------------------+
| renderer process (per tab)                  |
|   §39 profile: NO net, NO files (except fd  |
|   ipc), JIT permitted                       |
+---------------------------------------------+
```

## Failure paths

- **TLS handshake fail** → page shows "secure connection failed" overlay; chain inspection panel offered.
- **Renderer sandbox deny** → renderer killed; main spawns replacement; tab shows "tab crashed" UI.
- **GPU device lost** (Mesa) → renderer falls back to CPU Skia path; performance counter logs warning.
- **OOM in renderer** → tab killed, memory pressure event sent to compositor; main keeps running.
- **JIT cap denied** → LibJS falls back to interpreter; perf counter increments, banner not surfaced.

## Data structures

```rust
// kernel/src/mm/jit.rs
pub struct JitGrant {
    pub pid: Pid,
    pub addr: VirtAddr,
    pub len: usize,
    pub at_tick: TickInstant,
}

// userspace/qt-willo Wayland backend
pub struct QtWaylandSurface {
    pub xdg_surface: WlObject,
    pub dmabuf_pool: DmaBufPool,         // imported from Mesa
    pub viewport: Viewport,
}

// userspace/willo-browser/sandbox.rs
pub struct RendererProfile {
    pub allow_net: bool,                 // false in renderers
    pub allow_files: &'static [&'static str],
    pub allow_jit: bool,
}
```
