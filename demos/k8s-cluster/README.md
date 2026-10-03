# k8s-cluster demo

Boots a 3-node upstream Kubernetes cluster on qarax using kubeadm and standard
Fedora cloud images — no custom image build required.

## How it works

Each VM is a standard Fedora 43 Cloud image booted via Cloud Hypervisor UEFI
firmware (EDK2 `CLOUDHV.fd`, set via `VM_FIRMWARE` in `e2e/docker-compose.yml`).
On first run, `prebake.sh` installs containerd/kubeadm/kubelet and the
Kubernetes images into the base disk with `virt-customize`; cloud-init then
configures each node and runs `kubeadm init`/`join` at first boot.

The VMs run on the `qarax-node-2` container of the local Docker stack
(`make run-local`), which `run.sh` starts automatically if it is not running.

```
k8s-control-0  10.101.0.10  control-plane  2 vCPUs  4 GiB
k8s-worker-1   10.101.0.11  worker         2 vCPUs  3 GiB
k8s-worker-2   10.101.0.12  worker         2 vCPUs  3 GiB
```

Pod CIDR: `10.244.0.0/16` (Flannel).

## Prerequisites

- `docker`, `python3`, `kubectl`, `ssh-keygen` on the host
- `/dev/kvm` accessible
- Internet access (downloads the Fedora cloud image, k8s packages and images)
- Rust toolchain with `x86_64-unknown-linux-musl` target
- `QARAX_TOKEN` must match the server's `AUTH_TOKENS` (defaults to the local
  stack's `e2e-test-token`)

Passwordless `sudo` is optional: with it the demo creates a veth pair so VMs
are directly reachable; without it it uses `socat` relays inside the node
container.

## Usage

```bash
# Full run (first run downloads ~350 MB Fedora cloud image)
./demos/k8s-cluster/run.sh

# Tear down VMs and the Docker stack (including volumes, so the baked base
# image is rebuilt on the next run)
./demos/k8s-cluster/run.sh --cleanup
```

Re-running `run.sh` without `--cleanup` reuses the network, storage pool and
baked base image, and recreates the three VMs from scratch.

Tune with env vars (`CONTROL_PLANE_VCPUS`, `CONTROL_PLANE_MEMORY_MIB`,
`WORKER_VCPUS`, `WORKER_MEMORY_MIB`, `KUBERNETES_MINOR`, `KUBERNETES_VERSION`,
`COREDNS_VERSION`, `PAUSE_VERSION`, `ETCD_VERSION`, `FLANNEL_VERSION`):

```bash
CONTROL_PLANE_MEMORY_MIB=6144 ./demos/k8s-cluster/run.sh
```

When changing `KUBERNETES_MINOR`, also set `KUBERNETES_VERSION` (and the
CoreDNS/pause/etcd versions) to the ones that kubeadm release expects.

## Expected duration

| Phase                          | Time       |
|-------------------------------|------------|
| Stack start + base image DL   | 2-5 min    |
| Base image bake (first run)   | 10-15 min  |
| cloud-init k8s install (each) | 3-8 min    |
| All nodes Ready               | ~10-15 min |
| Smoke test                    | ~2 min     |

## How cloud-init installs Kubernetes

Templates in `cloud-init-control.sh` and `cloud-init-worker.sh` are rendered
by `run.sh` with a pre-generated kubeadm bootstrap token substituted in.
Each VM gets its own rendered user-data file so the control plane and workers
all configure their documented static IPs explicitly.

Workers poll `https://10.101.0.10:6443/healthz` before joining, so all three
VMs can be started simultaneously. Kubernetes images are imported from the
archive baked into the base disk; containerd is also pointed at the local
registry through a relay on the bridge gateway (`10.101.0.1:5000`), falling
back to upstream registries if that mirror is not reachable.

Once kubeadm init completes on the control plane, it serves the admin
kubeconfig on HTTP port 8080; `run.sh` downloads it and uses it for `kubectl`.

## Files

| File                    | Purpose                                     |
|------------------------|---------------------------------------------|
| `run.sh`               | Main orchestration script                   |
| `cloud-init-control.sh`| Setup script template for control plane     |
| `cloud-init-worker.sh` | Setup script template for workers           |
| `prebake.sh`           | Package install run in the base disk by `virt-customize` |
| `smoke.yaml`           | Smoke-test Deployment + NodePort Service    |
