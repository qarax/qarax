# Cloud Image Demo

Boot a VM from a stock cloud image (Ubuntu 22.04 minimal by default) with cloud-init
injecting your SSH key.

The script creates a local storage pool and a bridge network, downloads the cloud image
into the pool once, and copies it to a root disk of the VM's own (`qarax transfer
create`). It then creates and starts a UEFI (`--boot-mode firmware`) VM with that disk
as root and a cloud-init NoCloud seed. No OCI registry or OverlayBD is involved after
the download.

On first boot cloud-init creates a `qarax` user with your SSH key and starts a small
HTTP server on port 8080 that serves `demo-cloud-image`. The script waits up to 180 s
for that server to answer (probing from the qarax-node container) and checks the
response.

The VM (`demo-cloud-image-vm`) is left running. A re-run replaces the VM and its root
disk with a fresh copy of the downloaded image, and reuses the pool, the network and
the download.

## Prerequisites

- qarax stack running: `make run-local` (the script starts it if it is not running)
- `cargo` (the script builds the `qarax` CLI) or a `qarax` CLI on PATH
- `jq`, `curl`
- Internet access from qarax-node for the image download
- An SSH public key at `~/.ssh/id_ed25519.pub` or `~/.ssh/id_rsa.pub`, or set `SSH_PUB_KEY` / pass `--ssh-key`
- If your server uses a token other than the local default, export `QARAX_TOKEN`

## Usage

```bash
# Default: Ubuntu 22.04 minimal, 2 vCPUs, 1 GiB
./demos/cloud-image/run.sh

# Custom image (give it its own --disk-name so it doesn't reuse the Ubuntu download)
./demos/cloud-image/run.sh --image-url https://example.com/custom.img --disk-name custom-disk

# Bridge a host NIC so the VM gets an address on your LAN
./demos/cloud-image/run.sh --parent-interface eth0
```

Network flags (`--subnet`, `--gateway`, `--bridge-name`, `--parent-interface`) apply
when the network is created; run `--cleanup` first to change them.

## Options

| Flag | Default | Description |
|------|---------|-------------|
| `--image-url URL` | Ubuntu 22.04 minimal cloud image | Image to download into the pool |
| `--disk-name NAME` | `ubuntu-22.04-cloud` | Storage object name for the downloaded image |
| `--pool-name NAME` | `demo-cloud-image-pool` | Local storage pool name |
| `--pool-path PATH` | `/var/lib/qarax/storage/demo-cloud-image-pool` | Pool directory on the node (the default is on its persistent volume) |
| `--network-name NAME` | `demo-cloud-image-net` | Network name |
| `--subnet CIDR` | `10.97.0.0/24` | Network subnet |
| `--gateway IP` | `10.97.0.1` | Network gateway |
| `--bridge-name NAME` | `qci0` | Bridge device created on the host |
| `--parent-interface NIC` | — | Bridge this host NIC instead of an isolated NAT bridge |
| `--host NAME` | `$QARAX_HOST` or the first UP host | Host for the pool and network |
| `--vcpus N` | `2` | vCPU count |
| `--memory GiB` | `1` | Memory in GiB |
| `--ssh-key KEY` | `$SSH_PUB_KEY` or `~/.ssh/id_{ed25519,rsa}.pub` | SSH public key to inject |
| `--server URL` | `$QARAX_SERVER` or `http://localhost:8000` | qarax API URL |
| `--cleanup` | — | Delete the VM, its root disk and the network, then exit (keeps the download) |
| `--purge` | — | Like `--cleanup`, and also delete the downloaded image and the pool |

## Reaching the VM

In the Docker Compose stack the bridge lives inside the qarax-node container, so reach
the guest from there (the script prints the VM's IP):

```bash
docker compose -f e2e/docker-compose.yml exec qarax-node curl http://<VM_IP>:8080
```

With `--parent-interface`, the VM is on the upstream network and reachable directly:
`ssh qarax@<VM_IP>`.

On a host that is not a local compose node (e.g. `make run-local VM=1`) the script
can't reach the guest, so it skips the cloud-init check and prints the address instead.

## Cleanup

```bash
./demos/cloud-image/run.sh --cleanup   # keeps the downloaded image for the next run
./demos/cloud-image/run.sh --purge     # deletes everything
```

Pass the same names you used for the run if you overrode any defaults.
