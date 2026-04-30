# flow36 — M36: architecture & runtime flows (App suite, utilities)

> Derived from `docs/idea.md` §21 and the work36 plan.

## Component map

```
        +------------------+   +-----------------+
        | willoc-settings  |   |  willomonitor   |
        +------+-----------+   +-------+---------+
               |  plugin loader        | procfs/sysfs poll
               v                       v
        plugins (User, Net, Display,   1 Hz ringbuffer + graphs
        Sound, Power, Updates, Priv,
        A11y)

        +-------------+ +--------------+ +-------------+
        | willocalc   | | willoarchive | | willoshot   |
        +-------------+ +--------------+ +------+------+
                                                 |
                                                 v
                                        compositor screencopy

        +-------------+ +-----------------------------+
        | willoview   | |          willoplay          |
        +------+------+ +-------+----------+----------+
               |               | demux     | §40 codec
               v               v           v
            image-rs        mp4/mkv     dma-buf -> §22
            libavif

        +-------------------+
        | willoc-notify     |  (D-Bus notifications)
        +-------------------+
```

## willomonitor 1 Hz loop

```
every 1000 ms:
  read /proc/stat -> [user, nice, sys, idle, iowait, irq, softirq] per cpu
  read /proc/meminfo -> total/free/avail/buffers/cached
  for pid in /proc/*:
     read /proc/[pid]/stat -> utime, stime, rss
  read /proc/net/dev -> rx/tx bytes per iface
  read /sys/class/thermal/thermal_zone*/temp
  push sample to ringbuffer (capacity 300)
  emit "sample" signal -> graphs redraw
budget: < 5% CPU at idle
```

## Settings plugin loading

```
willoc-settings start
  -> read /usr/lib/willo-settings-plugins/*.toml
  -> for each plugin manifest:
       dlopen (or spawn child) plugin entry
       call SettingsPlugin::id() / title() / icon()
  -> render tab list
on tab activate:
  -> plugin renders its UI in embedded iced-willo surface
```

## Screenshot capture flow

```
willoshot --region
  -> compositor screencopy::capture(output, region)
       -> import compositor framebuffer dma-buf (M22)
       -> read into CPU buffer (only for region)
  -> show "annotation" overlay
       arrows / text / blur
  -> save:
       PNG encode (image-rs)
       write ~/Pictures/Screenshot_{ts}.png
       clipboard set image/png
```

## Image view + slideshow

```
willoview ~/Pictures/photo.avif
  -> decoder = match ext
       png -> image::png
       avif -> libavif
  -> decode → pixmap
  -> upload to GPU texture (§22) for fast pan/zoom
arrow-right -> next image (preload pipeline N+1)
F5 -> slideshow with crossfade
```

## Media player playback

```
willoplay movie.mp4
  -> demux (mp4) -> tracks: video(h264), audio(aac), subs(none)
  -> §40 willova::decode_video -> dma-buf frames
  -> §40 willova::decode_audio -> PCM
  -> A/V clock sync (audio master)
  -> for each frame at presentation time:
       compositor present(dmabuf) [zero-copy]
       audio mixer push PCM -> §18
  -> seek: flush; reset codecs to keyframe; resume
```

## Subtitle render

```
SRT loaded:
  parse cues -> [(start, end, text)]
ASS loaded:
  parse styles + events; track override codes
on each frame timestamp:
  active = cues whose [start,end] contains t
  for cue in active:
     render text (HarfBuzz §30) into overlay surface
     compose above video frame
```

## Notification toast

```
app sends D-Bus org.freedesktop.Notifications.Notify(
   summary="New mail", body="From: Alice", icon, urgency
)
willoc-notify:
  if quiet_hours -> queue silently
  else:
     create compositor toast surface (corner anchor)
     fade-in; show 5s; fade-out
     play sound if urgency >= NORMAL && settings.sound=on
```

## Failure paths

- **Plugin crash** (settings) → host marks tab "unavailable", others keep working.
- **procfs file missing** (kernel feature off) → graph shows "n/a"; no panic.
- **Codec missing for media file** → willoplay shows "Unsupported codec"; offers download via §19 if optional codec pack available.
- **Screencopy permission denied** (§39) → willoshot prompts user via portal; granted access cached.
- **Subtitle parse error** → falls back to no-subs; logs warning.

## Data structures

```rust
pub trait SettingsPlugin {
    fn id(&self) -> &str;
    fn title(&self) -> &str;
    fn icon(&self) -> IconHandle;
    fn render(&mut self, ctx: &mut UiCtx);
}

pub struct ProcSample {
    pub ts: u64,
    pub cpu: [CpuStats; MAX_CPUS],
    pub mem: MemStats,
    pub net: BTreeMap<String, NetStats>,
    pub temp: BTreeMap<String, i16>,     // millideg C
}

pub enum ImageFmt { Png, Jpeg, WebP, Gif, Avif, Bmp, Tiff }

pub struct Track {
    pub kind: TrackKind,                 // Video | Audio | Subtitle
    pub codec: Codec,
    pub timescale: u32,
    pub duration: u64,
}

pub struct Notification {
    pub id: u32,
    pub summary: String,
    pub body: String,
    pub icon: Option<IconHandle>,
    pub urgency: Urgency,                // Low | Normal | Critical
    pub timeout_ms: u32,
}
```
