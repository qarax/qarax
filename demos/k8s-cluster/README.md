# k8s-cluster demo

Boots a 3-node upstream Kubernetes cluster on qarax using kubeadm and standard
Fedora cloud images — no custom image build required.

## How it works

Each VM is a standard Fedora 43 Cloud image booted via Cloud Hypervisor UEFI
firmware (`rust-hypervisor-firmware`).  Kubernetes is installed entirely by
cloud-init at first boot.

```
k8s-control-0  10.101.0.10  control-plane  4 vCPUs  4 GiB
k8s-worker-1   10.101.0.11  worker         2 vCPUs  3 GiB
k8s-worker-2   10.101.0.12  worker         2 vCPUs  3 GiB
```

Pod CIDR: `10.244.0.0/16` (Flannel).

## Prerequisites

- `docker`, `jq`, `python3`, `nc` on the host
- `/dev/kvm` accessible
- Internet access (downloads Fedora cloud image and k8s packages at runtime)
- Rust toolchain with `x86_64-unknown-linux-musl` target

Passwordless `sudo` is optional: with it the demo creates a veth pair so VMs
are directly reachable; without it it uses `socat` relays inside the node
container.

## Usage

```bash
# Full run (first run downloads ~350 MB Fedora cloud image)
./demos/k8s-cluster/run.sh

# Tear down VMs and the Docker stack
./demos/k8s-cluster/run.sh --cleanup
```

Tune with env vars:

```bash
CONTROL_PLANE_MEMORY_MIB=6144 \
KUBERNETES_MINOR=1.33 \
./demos/k8s-cluster/run.sh
```

## Expected duration

| Phase                          | Time       |
|-------------------------------|------------|
| Stack start + base image DL   | 2-5 min    |
| cloud-init k8s install (each) | 3-8 min    |
| All nodes Ready               | ~10-15 min |
| Smoke test                    | ~2 min     |

## How cloud-init installs Kubernetes

Templates in `cloud-init-control.sh` and `cloud-init-worker.sh` are rendered
by `run.sh` with a pre-generated kubeadm bootstrap token substituted in.
Each VM gets its own rendered user-data file so the control plane and workers
all configure their documented static IPs explicitly.

Workers poll `https://10.101.0.10:6443/healthz` before joining, so all three
VMs can be started simultaneously. Guests prefer pulling images from the local
registry container's Docker-network IP, and fall back to upstream registries if
that mirror is not reachable.

Once kubeadm init completes on the control plane, it serves the admin
kubeconfig on HTTP port 8080; `run.sh` downloads it and uses it for `kubectl`.

## Files

| File                    | Purpose                                     |
|------------------------|---------------------------------------------|
| `run.sh`               | Main orchestration script                   |
| `cloud-init-control.sh`| Setup script template for control plane     |
| `cloud-init-worker.sh` | Setup script template for workers           |
| `smoke.yaml`           | Smoke-test Deployment + NodePort Service    |
