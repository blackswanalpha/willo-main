# flow40 — M40: architecture & runtime flows (Multimedia)

> Derived from `docs/idea.md` §11 and the work40 plan.

## Component map

```
        +-------------------+   +-----------------+
        |  willoplay (§36)  |   |  willo-browser  |
        +---------+---------+   +--------+--------+
                  |                      |
                  v                      v
        +-------------------------------------+
        |     userspace willova               |
        |  decoder/encoder API + plug-ins     |
        +----+--------+---------+--------+----+
             |        |         |        |
             v        v         v        v
            dav1d  openh264  libde265  libvpx       (SW codecs)
                       \\         //
                        v       v
        +------------------------+        +------------------+
        |  kernel willova        |<------>|  V4L2 capture     |
        |  (HW driver glue,       |        |  /dev/video*      |
        |   dma-buf surfaces)    |        +-------------------+
        +-----------+------------+
                    |
                    v
        +---------------+    +----------------+
        | virtio-gpu    |    |  Intel iGPU    |
        | video         |    |  (M22 hook)    |
        +---------------+    +----------------+

        +---------------+        +-----------------+
        |  willopipe    |<------>| compositor      |
        |  (screencast  |        | screencopy      |
        |   portal)     |        +--------+--------+
        +-------+-------+                 |
                |                         v
                v                  dma-buf surfaces (§22)
        client app subscribes → dma-buf frames

        +-----------------+
        |  willomidi      |  (ALSA-seq-class)
        +--------+--------+
                 |
                 v
        §18 audio core mixer -> HDA -> speakers
```

## Video decode flow (HW path)

```
willoplay opens video.mp4
  -> demux mp4 (ffmpeg-mux-class) -> tracks
  -> open willova decoder for AV1
       capabilities = HW || SW fallback
  -> per packet:
       willova::decode(packet, out_dmabuf)
         kernel: virtio-gpu video submit (HW)
                 or SW (dav1d on CPU; output dma-buf)
       fence signalled when frame ready
  -> compositor present(dmabuf, presentation_time)
  -> CPU memcpy counter == 0 (zero-copy)
```

## V4L2 capture flow

```
camera_app open("/dev/video0")
  ioctl VIDIOC_QUERYCAP -> "Willo Camera v1"
  ioctl VIDIOC_ENUM_FMT -> [NV12, MJPEG]
  ioctl VIDIOC_S_FMT     -> NV12 1280x720 30fps
  ioctl VIDIOC_REQBUFS   -> 4 buffers, type=DMABUF
  ioctl VIDIOC_QBUF      -> queue buf 0..3
  ioctl VIDIOC_STREAMON

driver:
  IRQ on frame ready
  fill dma-buf
  signal POLLIN

camera_app:
  poll() ready
  ioctl VIDIOC_DQBUF -> buf index N
  process / display via §22 dma-buf import
  ioctl VIDIOC_QBUF -> buf N (re-queue)
```

## willopipe screen capture

```
client app -> D-Bus org.freedesktop.portal.ScreenCast.CreateSession
willopipe portal:
  pop up "App X wants to capture: [Window | Output | Region]"
  user picks; sets persistence policy
  return session_handle
client -> SelectSources(session, options)
client -> Start(session, parent_window)
willopipe:
  set up compositor screencopy stream for selected source
  emit "Started" with stream_node + dma-buf format
client subscribes:
  per-frame dma-buf delivered until session ended
on session end:
  willopipe revokes; compositor stops capture; portal cleans state
```

## MIDI play flow

```
willomidi --play song.mid
  parse SMF (Standard MIDI File) -> events with delta-times
  open seq_client
  for each event at scheduled tick:
     if NoteOn:
        soft synth: synthesize PCM via §18 audio core
        push samples into mixer
     if NoteOff:
        damp/release
  end of track: drain
external USB-MIDI device:
  /dev/snd/midiC0D0 -> read events -> seq_client publishes
```

## Codec pack install

```
user opens video.mp4 with willoplay
  willova: H264 codec required; not installed
  willova: ask user "install codec pack? (proprietary)"
  user accepts -> §19 willo-pkg install willova-codecs-h264.willo
  willova reload plug-ins
  decode resumes
```

## A/V clock sync (willoplay)

```
audio is master:
  audio.queue.tail_pts -> current playback clock
video frame ready at pts P:
  if P > current_clock + threshold: schedule present at P
  if P < current_clock - threshold: drop frame (catch-up)
seek:
  flush demux + decoders
  reset clock to nearest keyframe pts
  resume
```

## Failure paths

- **HW decode unsupported** for codec → fall back to SW; perf counter logs warning.
- **dma-buf import fail** at compositor → fallback CPU blit (warns); next install rotation flagged.
- **V4L2 driver bug** (frame size mismatch) → driver returns EINVAL; app re-negotiates.
- **Portal denied** by user → ScreenCast.Start emits ErrCancelled.
- **MIDI underrun** → soft-synth emits silence frame; logged; not fatal.
- **Codec pack signing fail** → pkg install rejects (§38 trust root); user notified.

## Data structures

```rust
pub struct WillovaDecoder {
    pub codec: Codec,                    // H264 | HEVC | VP9 | AV1
    pub backend: DecoderBackend,         // VirtioGpu | IntelIgpu | Sw(dav1d|...)
    pub width: u32,
    pub height: u32,
    pub pix_fmt: PixFmt,                 // NV12 | I420 | P010
}

pub struct V4l2Buffer {
    pub index: u32,
    pub kind: V4l2BufType,               // Mmap | DmaBuf
    pub flags: V4l2BufFlags,
    pub timestamp_us: u64,
    pub bytes_used: u32,
    pub dma_buf: Option<DmaBufFd>,
}

pub struct PipeWireStream {
    pub session: SessionId,
    pub kind: StreamKind,                // Output | Window | Region
    pub fmt: VideoFmt,
    pub dma_buf_fmt: DmaBufFmt,
}

pub struct MidiEvent {
    pub tick: u32,
    pub kind: MidiKind,                  // NoteOn{ch,note,vel} | NoteOff | CC | ProgramChange | ...
}
```
