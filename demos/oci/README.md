# OCI VM Demo

Boot a VM directly from an OCI container image via OverlayBD.

Imports an image into the overlaybd storage pool, creates a VM, attaches the image as a disk, and starts it.
Re-running reuses an already-imported image; it refuses to overwrite an existing VM with the same name.

## Prerequisites

- qarax stack running: `make run-local` (the script starts it if it is not running).
  It registers the host and creates the `overlaybd-pool` storage pool.
- `qarax` CLI built (`make build`) or on PATH
- If your server uses a token other than the local default, export `QARAX_TOKEN`

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
| `--image REF` | `public.ecr.aws/docker/library/alpine:latest` | OCI image reference |
| `--object-name NAME` | derived from image (`alpine-latest-obd`) | Storage object name for the imported image |
| `--pool NAME` | `overlaybd-pool` | Storage pool name or ID |
| `--vcpus N` | `1` | vCPU count |
| `--memory MiB` | `256` | Memory in MiB |
| `--server URL` | `$QARAX_SERVER` or `http://localhost:8000` | qarax API URL |
| `--cleanup` | — | Delete the demo VM and imported storage object, then exit |

## Cleanup

```bash
./demos/oci/run.sh --cleanup
```

Pass the same `--name` / `--image` / `--object-name` you used for the run.
