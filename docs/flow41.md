# flow41 — M41: architecture & runtime flows (Containers / OCI)

> Derived from `docs/idea.md` §18 and the work41 plan.

## Component map

```
        userspace
        +------------------+   +------------------+
        | willopodman /    |   | willoc-containers|
        | willodocker      |   | (GUI)            |
        +--------+---------+   +--------+---------+
                 |                      |
                 v                      v
        +-------------------------------------------+
        |  willo-crun (OCI runtime)                 |
        |   spec.rs | run.rs | rootless.rs          |
        +--------+----------+-----------+-----------+
                 |          |           |
                 v          v           v
        clone3+ns     pivot_root   willo-image
        cgroup v2     mounts       (layers,
                                    overlay)
                 |
                 v
        +-------------------------+
        |  kernel ns + cgroup     |
        +-------------------------+
                 |
                 v
        netstack (incl. slirp4netns-class for rootless)
```

## Container start (rootless)

```
$ podman run --rm busybox echo hi
willopodman -> willo-crun create + start
  -> read /etc/subuid for $USER -> e.g. 100000:65536
  -> willo-image::ensure(busybox) -> layers + manifest cached
  -> assemble rootfs via overlay (M16):
       lowerdir = layer1:layer2:..., upperdir = container/upper, workdir = container/work
  -> clone3:
       flags = NEWUSER|NEWPID|NEWNS|NEWNET|NEWIPC|NEWUTS|NEWCGROUP|NEWTIME
  -> child:
       map uid 0 -> 100000 in user ns
       pivot_root rootfs
       mount /proc, /sys, /dev (devtmpfs subset)
       set cgroup memory.max, cpu.max, io.max
       apply §39 seccomp profile
       drop capabilities (per OCI spec)
       exec ["echo", "hi"]
  -> spawn slirp4netns-class helper for the new netns
  -> wait child; reap; print exit code
```

## cgroups v2 delegation

```
/sys/fs/cgroup/                 (root, all controllers)
  cgroup.subtree_control = +memory +cpu +io +pids
  user.slice/                   (logind delegated)
    user-1000.slice/            (per user)
      cgroup.subtree_control = +memory +cpu
      session-c1.scope/         (login session)
        app.slice/
          podman-pod-1.scope/   (rootless container)
            memory.max = 256M
```

## Rootless networking (slirp4netns-class)

```
container netns has only "lo" by default
willo-crun -> spawn slirp4netns child:
  - opens /dev/net/tun (in container ns) creating tap0
  - userspace TCP/IP stack (smoltcp-class)
  - host side: connect/bind via host netstack
  - inside container: tap0 has 10.0.2.100/24, gateway 10.0.2.2 (DNS too)
container traffic -> tap0 -> slirp daemon -> host netstack -> wlan0
```

## Image pull + cache

```
willo-image pull docker.io/library/busybox:latest
  -> token: GET /token?service=...&scope=repository:library/busybox:pull
  -> manifest: GET /v2/library/busybox/manifests/latest
       -> sha256
  -> blobs: GET /v2/library/busybox/blobs/sha256:<layer>
       -> store under /var/lib/willo-image/blobs/sha256/<a>/<b>/<sha>
  -> assemble layered rootfs on first run via overlay
```

## Stop + cleanup

```
container exit (or kill)
  -> willo-crun reaps PID
  -> tear down namespaces (refcount -> 0)
  -> unmount overlay; clear upperdir if --rm
  -> remove cgroup scope (atomically deletes cgroup.procs, charges return)
```

## Failure paths

- **uid map exhausted** → /etc/subuid range too small; willo-crun errors with clear remediation.
- **cgroup limit hit** (memory.max) → OOM-killer kills offending process inside container; host unaffected.
- **slirp4netns crash** → container loses net; runtime restarts helper; log warning.
- **overlay mount fail** (FS doesn't support) → fall back to copy-up bind mount (slow).
- **Image manifest mismatch** → pull aborted; cache untouched; user retry.

## Data structures

```rust
pub struct Namespace<K> {
    pub kind: NsKind,                    // Pid|Net|Mnt|Ipc|Uts|User|Cgroup|Time
    pub id: NsId,
    pub refcount: AtomicU32,
    pub data: Box<K>,                    // PidNs { numbering }, NetNs { ifaces }, ...
}

pub struct CgroupV2 {
    pub path: PathBuf,                    // relative to /sys/fs/cgroup
    pub controllers: ControllerMask,      // cpu|memory|io|pids|devices
    pub procs: HashSet<Pid>,
    pub limits: CgroupLimits,
}

pub struct OciSpec {
    pub oci_version: String,
    pub process: OciProcess,
    pub root: OciRoot,                    // path, readonly
    pub mounts: Vec<OciMount>,
    pub linux: OciLinux,                  // namespaces, resources, seccomp, capabilities, masked_paths
}

pub struct UidMap {
    pub container_id: u32,
    pub host_id: u32,
    pub size: u32,
}
```
