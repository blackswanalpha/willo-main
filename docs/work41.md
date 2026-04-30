# work41 — M41: Containers / OCI runtime (namespaces, cgroups v2, crun-class, rootless)

> Derived from `docs/idea.md` §18 (Virtualization & Containers — OCI containers + cgroups).

## Goal

Run unmodified Docker/Podman OCI images on Willo. Land Linux-shaped **namespaces** (uid, pid, net, mnt, ipc, uts, cgroup, time), a **cgroups v2** unified hierarchy with delegation, an OCI runtime spec parser + executor (`willo-crun`, crun/youki-class), an OCI image runtime, and **rootless** mode using slirp4netns-class user-mode networking.

## Depends on

- **M11/M12** — process model + signals + IPC.
- **M14/M21/M42** — netstack + wlan + WireGuard (for in-container nets).
- **M16** — VFS + WilloFS overlay mounts.
- **M37** — multi-user (rootless requires user namespaces + uid mapping).
- **M39** — sandboxing (default profile per container).

## Acceptance criteria

- [ ] `unshare(CLONE_NEW{PID,NET,NS,IPC,UTS,USER,CGROUP,TIME})` works as Linux-equivalent.
- [ ] cgroups v2 unified tree at `/sys/fs/cgroup/` with `cgroup.subtree_control` delegation.
- [ ] `willo-crun run config.json` honors OCI runtime spec: rootfs, mounts, process, linux.namespaces, linux.resources.
- [ ] `podman run --rm hello-world` runs (rootless preferred).
- [ ] Rootless mode: per-user `unshare(CLONE_NEWUSER)`, slirp4netns-class user-mode net.
- [ ] CPU/memory/IO/PID/device controllers enforce per-container limits.
- [ ] OCI image runtime pulls + caches a small image (busybox).
- [ ] `kernel/tests/cgroup_v2_basic.rs` and `kernel/tests/oci_run_busybox.rs` pass.

## Task breakdown

### T1. Namespaces — `kernel/src/ns/`
- Per-namespace types: pid, net, mnt, ipc, uts, user, cgroup, time.
- `unshare`, `setns`, `clone3` syscalls; reference-counted; cleaned up at last task exit.

### T2. cgroups v2 — `kernel/src/cgroup/`
- Unified hierarchy at `/sys/fs/cgroup/`.
- Controllers: cpu, memory, io, pids, devices.
- Delegation via `cgroup.subtree_control`; never violate "no internal processes" rule.

### T3. OCI runtime spec — `userspace/willo-crun/spec.rs`
- Parse `config.json`; validate against OCI Runtime Spec v1.1.
- Translate to namespace flags, mount list, cgroup limits, capabilities, seccomp profile.

### T4. willo-crun executor — `userspace/willo-crun/run.rs`
- `clone3` with namespaces; `pivot_root` into rootfs; apply mounts; drop caps; apply §39 seccomp.
- Wait + reap child; relay exit code.

### T5. OCI image runtime — `userspace/willo-image/`
- Pull from OCI registry (HTTPS + token bearer auth).
- Verify manifest signature (cosign-class — defer to follow-up).
- Layer extraction into `/var/lib/willo-image/layers/<sha>/`.
- Overlay mount per container.

### T6. Rootless mode — `userspace/willo-crun/rootless.rs`
- newuidmap/newgidmap-class subuid/subgid mapping in `/etc/subuid`, `/etc/subgid`.
- `slirp4netns`-class user-mode network process per container.

### T7. UI integration — `userspace/willoc-containers/`
- Compositor client: list/run/inspect/stop containers; reuses §25 Boxes UI patterns.

### T8. CLI shims — `userspace/willodocker/`, `userspace/willopodman/`
- Thin wrappers expressing Docker/Podman commands in willo-crun + willo-image terms (subset v1).

## New / modified files

| Path | Change |
| --- | --- |
| `kernel/src/ns/*` | **new** namespaces module |
| `kernel/src/cgroup/*` | **new** cgroups v2 |
| `kernel/src/syscall.rs` | + unshare/setns/clone3 |
| `userspace/willo-crun/` | **new** |
| `userspace/willo-image/` | **new** |
| `userspace/willodocker/` | **new** wrapper |
| `userspace/willopodman/` | **new** wrapper |
| `userspace/willoc-containers/` | **new** GUI |

## Tests to add

- `kernel/tests/cgroup_v2_basic.rs` — create subtree, set memory.max, run a child that OOMs.
- `kernel/tests/ns_pid_isolation.rs` — pid namespace shows pid 1 inside.
- `kernel/tests/ns_net_isolation.rs` — net namespace gets its own loopback only.
- `kernel/tests/oci_run_busybox.rs` — `willo-crun run` busybox image; capture stdout.
- `userspace/willo-crun/tests/rootless_uid_map.rs` — uid mapping correct.

## Risks & open questions

- **cgroups v2 delegation footguns** — children inherit only what `subtree_control` exposes; document with examples.
- **slirp4netns-class throughput** — slow vs veth; acceptable v1; promote to CNI bridge in M41.x.
- **OCI image signing** — cosign deferred; v1 ships unsigned with prominent warning.
- **Linux compat overlap (§26)** — containers run native Willo binaries by default; Linux ELF inside container goes through §26 syscall path.
- **§39 default profile** — every container gets a tightened MAC/seccomp profile; tunable in config.json `linux.seccomp`.
- **Kernel module scope creep** — overlayfs is a real chunk of work; share code with §16 WilloFS snapshot.
