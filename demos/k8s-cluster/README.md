# k8s-cluster demo

Boots a 3-node upstream Kubernetes cluster on qarax with kubeadm and the
standard Fedora Cloud image, configured entirely through cloud-init. The
cluster stays running when the script ends.

## How it works

1. **Second node.** The VMs run on the local stack's `qarax-node-2`
   container. `run.sh` starts and registers it if needed (`ensure_up_hosts 2`
   in `demos/lib.sh`), so the cluster doesn't take memory from the default
   node the other demos use.
2. **Network and pool.** A qarax network `demo-k8s-net` (bridge, DHCP and NAT
   on the node) and a local storage pool `demo-k8s-pool`, created on the first
   run and reused afterwards.
3. **Base image, built once.** `prebake.sh` runs inside `virt-customize` on
   the Fedora 43 Cloud image: it installs containerd and the Kubernetes
   packages for `KUBERNETES_MINOR`, then saves every image the cluster runs
   (`kubeadm config images list` for the installed version, plus the images
   in the Flannel manifest) into the disk, so the nodes boot without pulling.
   The result is named after a hash of `prebake.sh`, the Fedora image URL,
   `KUBERNETES_MINOR` and `FLANNEL_VERSION`, and is cached (see below).
4. **Disks.** Each VM gets a blank 20 GiB raw disk from
   `qarax storage-pool create-disk`, and the base image is written straight
   into it; the three copies run in parallel.
5. **VMs.** `qarax vm create --boot-mode firmware` (UEFI, EDK2 `CLOUDHV.fd`)
   with a static IP and per-VM cloud-init user-data, then all three start at
   once. On first boot `node-setup.sh` configures the network, imports the
   baked images and runs `kubeadm init` (control plane, then Flannel) or
   `kubeadm join` (workers, which wait for the API server first).
6. **Waiting.** `run.sh` polls each VM's setup status over SSH every 5 s and
   stops at the first failure, printing the tail of the guest's
   `/var/log/k8s-setup.log`. It also fails as soon as a VM leaves `running`.
7. **Proof.** It fetches the admin kubeconfig over SSH, waits for all three
   nodes to be `Ready`, deploys `smoke.yaml` (2 replicas of
   `nginxdemos/hello` behind NodePort 30080, pulled from Docker Hub through the
   node's NAT) and fetches a page from it.

```
VM                   IP           role           vCPUs  memory
demo-k8s-control-0   10.101.0.10  control plane  2      4 GiB
demo-k8s-worker-1    10.101.0.11  worker         2      3 GiB
demo-k8s-worker-2    10.101.0.12  worker         2      3 GiB
```

Gateway and NAT: `10.101.0.1`. Pod CIDR: `10.244.0.0/16` (Flannel).

### Reaching the VMs

With passwordless `sudo`, `run.sh` plugs a veth pair from the host into the
node's bridge (`veth-k8s`, host address `10.101.0.254`), so the VMs, the API
server and the NodePort are reachable at their own addresses. Without it, it
starts `socat` relays inside the node container instead: API server on port
36443, NodePort on 36080, SSH to the VMs on 32210-32212, all at the
container's Docker IP. The kubeconfig is rewritten to match.

Each VM has a `qarax-debug` user (passwordless sudo) with the demo's SSH key;
`run.sh` prints the `ssh` command for each VM. `qarax vm attach <vm>` logs
that user in on the serial console.

## Prerequisites

- `docker` (with Compose), `jq`, `kubectl`, `ssh`, `ssh-keygen` and `curl` on
  the host
- `/dev/kvm`
- Internet access: the Fedora image (~0.5 GB) and the Kubernetes packages and
  images on the first run, the smoke-test image on every run
- Memory for the VMs (10 GiB with the defaults, tunable below), and disk:
  qarax's scheduler wants 21 GiB free per 20 GiB root disk, and the cached
  base image is ~2-3 GB in both `~/.cache` and the node's volume
- `QARAX_TOKEN` matching the server's `AUTH_TOKENS` (defaults to the local
  stack's `e2e-test-token`)
- Passwordless `sudo` is optional (see above)

## Usage

```bash
# Create the cluster (starts the local stack and qarax-node-2 if needed)
./demos/k8s-cluster/run.sh

export KUBECONFIG=~/.cache/qarax/k8s-cluster/admin.conf
kubectl get nodes

# Delete the VMs, disks, network, storage pool and host veth/relays
./demos/k8s-cluster/run.sh --cleanup

# Same, plus the cached base image and the demo's SSH key
./demos/k8s-cluster/run.sh --purge
```

Re-running `run.sh` deletes the previous run's VMs and disks (everything named
`demo-k8s-*`) and builds a fresh cluster on the existing network and pool.

Tune with environment variables:

| Variable | Default |
|----------|---------|
| `KUBERNETES_MINOR` | `1.35` (the newest patch release of it is installed) |
| `FLANNEL_VERSION` | `v0.28.1` |
| `CONTROL_PLANE_VCPUS` / `CONTROL_PLANE_MEMORY_MIB` | `2` / `4096` |
| `WORKER_VCPUS` / `WORKER_MEMORY_MIB` | `2` / `3072` |

```bash
CONTROL_PLANE_MEMORY_MIB=3072 WORKER_MEMORY_MIB=2048 ./demos/k8s-cluster/run.sh
```

Changing `KUBERNETES_MINOR` or `FLANNEL_VERSION` bakes a new base image; the
image versions follow from them.

## Caches and state

| Where | What |
|-------|------|
| `~/.cache/qarax/k8s-cluster/` (`$XDG_CACHE_HOME`) | baked base image, SSH key, `admin.conf`, rendered user-data |
| `qarax-node-2:/var/lib/qarax/storage/demo-k8s-cache/` | Fedora download and the baked base image `qemu-img` copies from |

The node copy goes away with the stack's volumes; the next run restores it
from `~/.cache` instead of baking again. Only one baked image is kept in each
place.

## Expected duration

| Phase | Time |
|-------|------|
| Base image bake (first run, or a new version) | 10-15 min |
| Disks | about a minute |
| Boot, `kubeadm init` and the two joins | a few minutes |
| Nodes Ready and smoke test | 1-2 min |

## Files

| File | Purpose |
|------|---------|
| `run.sh` | Orchestration |
| `prebake.sh` | Base image customisation, run by `virt-customize` |
| `node-setup.sh` | First-boot setup for both roles, run by cloud-init |
| `smoke.yaml` | Smoke-test Deployment and NodePort Service |
