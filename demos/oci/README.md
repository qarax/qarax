# OCI VM Demo

Boot a VM directly from an OCI container image via OverlayBD.

Imports an image into the overlaybd storage pool, creates a VM, attaches the image as a disk, and starts it.

## Prerequisites

- qarax stack running: `./hack/run-local.sh`
- `qarax` CLI on PATH

## Usage

```bash
# Default: Alpine Linux
./demos/oci/run.sh

# Custom image
./demos/oci/run.sh --image docker.io/library/ubuntu:latest --name ubuntu-vm

# More resources
./demos/oci/run.sh --vcpus 2 --memory 512
```

## Options

| Flag | Default | Description |
|------|---------|-------------|
| `--name NAME` | `demo-oci-vm` | VM name |
| `--image REF` | `alpine:latest` | OCI image reference |
| `--pool NAME` | `overlaybd-pool` | Storage pool name |
| `--vcpus N` | `1` | vCPU count |
| `--memory MiB` | `256` | Memory in MiB |
| `--server URL` | `$QARAX_SERVER` | qarax API URL |

## Cleanup

```bash
qarax vm stop demo-oci-vm
qarax vm delete demo-oci-vm
```
