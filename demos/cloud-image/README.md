# Cloud Image Demo

Boot a VM from a stock cloud image (Ubuntu 22.04 minimal by default) with cloud-init
injecting your SSH key.

Creates a local storage pool and a bridge network, downloads the cloud image into the
pool as a standalone raw disk, then creates and starts a UEFI (`--boot-mode firmware`)
VM with that disk as root and a cloud-init NoCloud seed. No OCI registry or OverlayBD
is involved after the download.

On first boot cloud-init creates a `qarax` user with your SSH key and starts a small
HTTP server on port 8080 that serves `demo-cloud-image`.

Re-running reuses the pool, network, and downloaded disk; it refuses to overwrite an
existing VM with the same name.

## Prerequisites

- qarax stack running: `make run-local` (the script starts it if it is not running)
- `qarax` CLI built (`make build`) or on PATH
- `jq`
- Internet access from qarax-node for the image download
- An SSH public key at `~/.ssh/id_ed25519.pub` or `~/.ssh/id_rsa.pub`, or set `SSH_PUB_KEY` / pass `--ssh-key`
- If your server uses a token other than the local default, export `QARAX_TOKEN`

## Usage

```bash
# Default: Ubuntu 22.04 minimal, 2 vCPUs, 1 GiB
./demos/cloud-image/run.sh

# Custom image and VM name
./demos/cloud-image/run.sh --image-url https://example.com/custom.img --disk-name custom-disk --name my-vm

# Preallocate disk blocks instead of a sparse file
./demos/cloud-image/run.sh --preallocate

# Bridge a host NIC so the VM gets an address on your LAN
./demos/cloud-image/run.sh --parent-interface eth0
```

## Options

| Flag | Default | Description |
|------|---------|-------------|
| `--name NAME` | `demo-cloud-image-vm` | VM name |
| `--image-url URL` | Ubuntu 22.04 minimal cloud image | Image to download into the pool |
| `--disk-name NAME` | `ubuntu-22.04-cloud` | Disk storage object name |
| `--preallocate` | off (sparse) | Reserve disk blocks upfront |
| `--pool-name NAME` | `demo-cloud-image-pool` | Local storage pool name |
| `--pool-path PATH` | `/var/lib/qarax/cloud-image-pool` | Pool directory on qarax-node |
| `--network-name NAME` | `demo-cloud-image-net` | Network name |
| `--subnet CIDR` | `10.97.0.0/24` | Network subnet |
| `--gateway IP` | `10.97.0.1` | Network gateway |
| `--bridge-name NAME` | `qci0` | Bridge device created on the host |
| `--parent-interface NIC` | — | Bridge this host NIC instead of an isolated NAT bridge |
| `--host NAME` | `$QARAX_HOST` or `local-node` | Host for the pool and network |
| `--vcpus N` | `2` | vCPU count |
| `--memory GiB` | `1` | Memory in GiB |
| `--ssh-key KEY` | `$SSH_PUB_KEY` or `~/.ssh/id_{ed25519,rsa}.pub` | SSH public key to inject |
| `--server URL` | `$QARAX_SERVER` or `http://localhost:8000` | qarax API URL |
| `--cleanup` | — | Delete the VM, disk, network, and pool, then exit |

## Reaching the VM

In the Docker Compose stack the bridge lives inside the qarax-node container, so test
from there (the script prints the allocated IP):

```bash
docker compose -f e2e/docker-compose.yml exec qarax-node curl http://<VM_IP>:8080
```

With `--parent-interface`, the VM is on the upstream network and reachable directly:
`ssh qarax@<VM_IP>`.

## Cleanup

```bash
./demos/cloud-image/run.sh --cleanup
```

Pass the same names you used for the run if you overrode any defaults.
