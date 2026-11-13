# Boot Source Demo

Boot a VM from a kernel + initramfs using the traditional direct-boot workflow.

The script creates a local storage pool, copies the kernel and initramfs into it
(`qarax transfer create`), assembles a boot source, and creates and starts a VM with
the guest agent enabled. It then runs `uname -a` and `cat /proc/cmdline` in the guest
(`qarax vm exec`) to show the VM booted with the boot source's kernel command line.

The VM (`demo-bootsrc-vm`) is left running. A re-run replaces the VM, the boot source
and the copied kernel/initramfs (so `--kernel`, `--initramfs` and `--cmdline` always
take effect) and reuses the pool.

## Prerequisites

- qarax stack running: `make run-local` (the script starts it if it is not running).
  The local qarax-node image ships the kernel (`/var/lib/qarax/images/vmlinux`) and a
  test initramfs (`/var/lib/qarax/images/test-initramfs.gz`) used as defaults.
- `cargo` (the script builds the `qarax` CLI) or a `qarax` CLI on PATH
- `jq`
- If your server uses a token other than the local default, export `QARAX_TOKEN`

## Usage

```bash
# Default: built-in kernel + test initramfs
./demos/boot-source/run.sh

# Custom kernel command line
./demos/boot-source/run.sh --cmdline "console=ttyS0 quiet"

# Custom kernel and initramfs
./demos/boot-source/run.sh --kernel /path/to/vmlinux --initramfs /path/to/initramfs.gz
```

Kernel and initramfs paths refer to files on the qarax-node host (inside the
qarax-node container for the Docker stack), not on your workstation.

The guest check talks to the guest agent, which is qarax-init in the default test
initramfs. With `--no-initramfs`, or an initramfs that does not run qarax-init, that
step fails.

## Options

| Flag | Default | Description |
|------|---------|-------------|
| `--pool-name NAME` | `demo-bootsrc-pool` | Local storage pool name |
| `--pool-path PATH` | `/var/lib/qarax/images` | Pool directory on the host |
| `--host NAME` | `$QARAX_HOST` or the first UP host | Host to attach the pool to |
| `--kernel PATH` | `/var/lib/qarax/images/vmlinux` | Kernel path on the host |
| `--initramfs PATH` | `/var/lib/qarax/images/test-initramfs.gz` | Initramfs path on the host |
| `--no-initramfs` | — | Boot without an initramfs |
| `--cmdline PARAMS` | `console=ttyS0` | Kernel command line |
| `--vcpus N` | `1` | vCPU count |
| `--memory MiB` | `256` | Memory in MiB |
| `--server URL` | `$QARAX_SERVER` or `http://localhost:8000` | qarax API URL |
| `--cleanup` | — | Delete the VM, boot source, storage objects, and pool, then exit |

## Cleanup

```bash
./demos/boot-source/run.sh --cleanup
```

Pass the same `--pool-name` you used for the run if you overrode it.
