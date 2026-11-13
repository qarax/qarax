# OCI VM Demo

Boot a VM directly from an OCI container image via OverlayBD.

The script imports an image into the `overlaybd-pool` storage pool, creates a VM with
the image as its boot disk (`--root-disk`, so it lands on a host that has the pool) and
the guest agent enabled, and starts it. It then
runs `cat /etc/os-release` in the guest (`qarax vm exec`) to show the VM booted from
the image.

The VM (`demo-oci-vm`) is left running. A re-run replaces it and reuses the already
imported image.

## Prerequisites

- qarax stack running: `make run-local` (the script starts it if it is not running).
  It registers the host and creates the `overlaybd-pool` storage pool.
- `cargo` (the script builds the `qarax` CLI) or a `qarax` CLI on PATH
- `jq`
- If your server uses a token other than the local default, export `QARAX_TOKEN`

## Usage

```bash
# Default: Alpine Linux
./demos/oci/run.sh

# Custom image (the guest check reads /etc/os-release, so the image needs one)
./demos/oci/run.sh --image docker.io/library/ubuntu:latest

# More resources
./demos/oci/run.sh --vcpus 2 --memory 512
```

## Options

| Flag | Default | Description |
|------|---------|-------------|
| `--image REF` | `public.ecr.aws/docker/library/alpine:latest` | OCI image reference |
| `--object-name NAME` | derived from image (`alpine-latest-obd`) | Storage object name for the imported image |
| `--pool NAME` | `overlaybd-pool` | OverlayBD storage pool name or ID |
| `--vcpus N` | `1` | vCPU count |
| `--memory MiB` | `256` | Memory in MiB |
| `--server URL` | `$QARAX_SERVER` or `http://localhost:8000` | qarax API URL |
| `--cleanup` | — | Delete the demo VM and the imported storage object, then exit |

## Cleanup

```bash
./demos/oci/run.sh --cleanup
```

Pass the same `--image` / `--object-name` you used for the run so the right storage
object is deleted.
