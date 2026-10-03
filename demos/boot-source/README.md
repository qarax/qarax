# Boot Source Demo

Boot a VM from a kernel + initramfs using the traditional direct-boot workflow.

Creates a local storage pool, transfers kernel/initramfs into it, assembles a boot source, and starts a VM.
Re-running reuses the pool and storage objects; it refuses to overwrite an existing VM with the same name.

## Prerequisites

- qarax stack running: `make run-local` (the script starts it if it is not running).
  The local qarax-node image ships the kernel (`/var/lib/qarax/images/vmlinux`) and a
  test initramfs (`/var/lib/qarax/images/test-initramfs.gz`) used as defaults.
- `qarax` CLI built (`make build`) or on PATH
- If your server uses a token other than the local default, export `QARAX_TOKEN`

## Usage

```bash
# Default: built-in kernel + test initramfs
./demos/boot-source/run.sh

# Custom kernel, no initramfs
./demos/boot-source/run.sh --kernel /path/to/vmlinux --no-initramfs

# Custom kernel cmdline
./demos/boot-source/run.sh --cmdline "console=ttyS0 root=/dev/vda rw"
```

Kernel and initramfs paths refer to files on the qarax-node host (inside the
qarax-node container for the Docker stack), not on your workstation.

## Options

| Flag | Default | Description |
|------|---------|-------------|
| `--name NAME` | `demo-bootsrc-vm` | VM name |
| `--pool-name NAME` | `demo-local-pool` | Local storage pool name |
| `--pool-path PATH` | `/var/lib/qarax/images` | Pool directory on qarax-node |
| `--host NAME` | `$QARAX_HOST` or `local-node` | Host to attach the pool to |
| `--kernel PATH` | `/var/lib/qarax/images/vmlinux` | Kernel path on qarax-node |
| `--initramfs PATH` | `/var/lib/qarax/images/test-initramfs.gz` | Initramfs path on qarax-node |
| `--no-initramfs` | — | Skip initramfs |
| `--cmdline PARAMS` | `console=ttyS0` | Kernel command line |
| `--vcpus N` | `1` | vCPU count |
| `--memory MiB` | `256` | Memory in MiB |
| `--server URL` | `$QARAX_SERVER` or `http://localhost:8000` | qarax API URL |
| `--cleanup` | — | Delete the VM, boot source, storage objects, and pool, then exit |

## Cleanup

```bash
./demos/boot-source/run.sh --cleanup
```
