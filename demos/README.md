# qarax Demos

Each demo lives in its own directory with a `run.sh` and a `README.md`.

## Single-host demos (local stack)

Start the stack once with `make run-local` (or `./hack/run-local.sh`), then run
any of these.

| Demo | Description | Stack required |
|------|-------------|----------------|
| [oci/](oci/) | Boot a VM from an OCI container image via OverlayBD | `./hack/run-local.sh` |
| [boot-source/](boot-source/) | Boot a VM from a kernel + initramfs | `./hack/run-local.sh` |
| [cloud-image/](cloud-image/) | Boot a VM from a cloud image (qcow2/raw) with cloud-init SSH key injection | `./hack/run-local.sh` + internet |
| [hooks/](hooks/) | Watch lifecycle webhook notifications fire in real time | `./hack/run-local.sh` |
| [sse-events/](sse-events/) | Stream VM status changes over Server-Sent Events (`GET /events`) | `./hack/run-local.sh` |
| [vm-exec/](vm-exec/) | Create a guest-agent-enabled VM and run a command inside the guest | `./hack/run-local.sh` |
| [vm-commit/](vm-commit/) | Convert an OverlayBD (OCI-backed) VM into a standalone raw-disk VM | `./hack/run-local.sh` |
| [backups/](backups/) | VM and control-plane database backups via `qarax backup` | `./hack/run-local.sh` |
| [network-isolation/](network-isolation/) | Same-VPC subnet routing plus VM security groups with live firewall updates | `./hack/run-local.sh` |
| [sandbox/](sandbox/) | Ephemeral VMs from templates with idle-timeout auto-reap and prewarmed pool claims | `./hack/run-local.sh` |
| [firecracker/](firecracker/) | Firecracker backend lifecycle (create/start/pause/resume/stop/delete) | `./hack/run-local.sh` |
| [block-storage/](block-storage/) | Register an iSCSI (LIO) target as a shared `BLOCK` storage pool | `./hack/run-local.sh` + Docker |
| [gpu-passthrough/](gpu-passthrough/) | GPU passthrough via VFIO to an OCI-booted VM | `./hack/run-local.sh` + VFIO GPU |

## Two-host demos

These need two registered hosts. `make run-local` already starts the second
node container (`qarax-node-2`) but only registers the first, so register it:

```bash
make run-local
QARAX_TOKEN=${QARAX_TOKEN:-e2e-test-token} bash e2e/setup_host.sh http://localhost:8000 qarax-node-2 50051 local-node-2
```

Alternatively, keep the stack from a two-node e2e run:

```bash
cd e2e && KEEP=1 ./run_e2e_tests.sh test_live_migration.py::test_host_evacuation_marks_maintenance_and_avoids_rescheduling
```

| Demo | Description |
|------|-------------|
| [cross-host-vpc/](cross-host-vpc/) | Cross-host same-VPC routing plus live security-group updates across two hosts |
| [host-evacuation/](host-evacuation/) | Move a VM off a host, leave it in maintenance, and prove new scheduling avoids it |

## Self-contained demos

These bring up their own environment.

| Demo | Description | Requirements |
|------|-------------|--------------|
| [etcd-cluster/](etcd-cluster/) | 3-node etcd cluster, each node as a VM | Docker + podman + KVM |
| [k8s-cluster/](k8s-cluster/) | Upstream 3-node Kubernetes cluster via kubeadm on VMs | Docker + podman + KVM |
| [hyperconverged/](hyperconverged/) | Control plane running inside a Cloud Hypervisor VM on bare metal (workload VMs default to `passt`) | KVM + podman + root |

## API authentication

The local stack enables API token auth (`AUTH_ENABLED=true` in
`e2e/docker-compose.yml`) with the token `e2e-test-token`, or
`$QARAX_TEST_TOKEN` if you set it. The demos default `QARAX_TOKEN` to the same
value, so they work out of the box. To point a demo at a server with a different
token:

```bash
export QARAX_TOKEN=<your-token>
QARAX_SERVER=http://my-server:8000 ./demos/oci/run.sh
```

When calling the API by hand, pass the token as a bearer header:

```bash
curl -H "Authorization: Bearer $QARAX_TOKEN" http://localhost:8000/vms
```

## Networking notes

- `hyperconverged/` defaults to `passt` for its workload VMs to avoid extra
  bridge/DHCP/NAT setup in the nested environment.
- `etcd-cluster/` and `k8s-cluster/` use bridged Qarax-managed networks because
  they depend on multi-VM reachability and static guest IPs.

## Writing a new demo

Source the shared helpers instead of redefining them:

```bash
REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
source "${REPO_ROOT}/demos/lib.sh"

SERVER="${QARAX_SERVER:-http://localhost:8000}"
require_server "$SERVER"            # reachable + token accepted (or ensure_stack to auto-start)
QARAX="$(find_qarax_bin)"           # newest local CLI build, or `qarax` on PATH
api_curl -sf "${SERVER}/vms" | jq . # curl with the bearer token attached
```

## Cleanup

```bash
# Stop individual VMs
qarax vm stop <name>
qarax vm delete <name>

# Tear down the entire stack
./hack/run-local.sh --cleanup
```
