# Boot Source Demo

Boot a VM from a kernel + initramfs using the traditional direct-boot workflow.

Creates a local storage pool, transfers kernel/initramfs into it, assembles a boot source, and starts a VM.

## Prerequisites

- qarax stack running: `./hack/run-local.sh --with-vm` (builds kernel and initramfs)
- `qarax` CLI on PATH

## Usage

```bash
# Default: uses kernel/initramfs built by run-local.sh --with-vm
./demos/boot-source/run.sh

# Custom kernel
./demos/boot-source/run.sh --kernel /path/to/vmlinux --no-initramfs

# Custom kernel cmdline
./demos/boot-source/run.sh --cmdline "console=ttyS0 root=/dev/vda rw"
```

## Options

| Flag | Default | Description |
|------|---------|-------------|
| `--name NAME` | `demo-bootsrc-vm` | VM name |
| `--kernel PATH` | `/var/lib/qarax/images/vmlinux` | Kernel path on qarax-node |
| `--initramfs PATH` | `boot-initramfs.gz` | Initramfs path on qarax-node |
| `--no-initramfs` | — | Skip initramfs |
| `--cmdline PARAMS` | `console=ttyS0` | Kernel command line |
| `--vcpus N` | `1` | vCPU count |
| `--memory MiB` | `256` | Memory in MiB |
| `--server URL` | `$QARAX_SERVER` | qarax API URL |

## Cleanup

```bash
qarax vm stop demo-bootsrc-vm
qarax vm delete demo-bootsrc-vm
```
