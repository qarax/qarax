# qarax-init

Minimal PID 1 init process for VMs that boot from OCI container images. Allows containers -- including scratch/distroless images with no userspace tools -- to run as Cloud Hypervisor VMs.

## What it does

When the kernel starts qarax-init as PID 1 inside a VM:

1. Mounts `/proc`, `/sys`, `/dev`
2. Creates `/dev/fd`, `/dev/stdin`, `/dev/stdout`, `/dev/stderr` symlinks
3. If running from an initramfs, mounts the real root device (from the `root=` kernel param) and performs switch_root
4. Brings up the loopback interface (raw ioctls, no `ip` binary needed)
5. Reads `/.qarax-config.json` for entrypoint, cmd, and env from the OCI image config
6. Forks, execs the workload in the child, and reaps zombies in the parent

Logs to both `/dev/kmsg` and stderr.

## Building

```bash
cargo build -p qarax-init --release --target x86_64-unknown-linux-musl
```

Must be a static musl binary since it runs in minimal VM environments.

## Configuration

qarax-init reads two sources:

**`/.qarax-config.json`** -- placed by qarax-node when preparing the VM rootfs:

```json
{
  "entrypoint": ["/usr/bin/myapp"],
  "cmd": ["--flag"],
  "env": ["KEY=value"]
}
```

All fields are optional. Defaults to `/bin/sh` if both entrypoint and cmd are empty.

**Kernel command line** (parsed from `/proc/cmdline`):

- `root=DEVICE` -- root block device to mount when booting from initramfs (e.g. `root=/dev/vda`)
- `rootfstype=TYPE` -- filesystem type for root device (default: `ext4`)

## How it gets into a VM

qarax-node injects qarax-init into each OCI-booted VM's filesystem before boot:

- **virtiofs path:** copies the binary to `/.qarax-init` in the overlayfs upper layer
- **OverlayBD path:** temporarily mounts the block device, copies the binary, unmounts

The control plane sets `init=/.qarax-init` on the kernel command line so the kernel runs it as PID 1.

The qarax-node `--qarax-init-binary` flag controls where the binary is read from (default: `/usr/local/bin/qarax-init`). If absent, OCI init injection is disabled.
