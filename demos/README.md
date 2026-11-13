# qarax Demos

Each demo lives in its own directory with a `run.sh` and a `README.md`.

## Single-host demos (local stack)

Start the stack once with `make run-local` (or `./hack/run-local.sh`), then run
any of these. A demo also starts the stack itself if it isn't running.

| Demo | Description | Stack required |
|------|-------------|----------------|
| [oci/](oci/) | Boot a VM from an OCI container image via OverlayBD | `./hack/run-local.sh` |
| [boot-source/](boot-source/) | Boot a VM from a kernel + initramfs | `./hack/run-local.sh` |
| [cloud-image/](cloud-image/) | Boot a VM from a cloud image (qcow2/raw) with cloud-init SSH key injection | `./hack/run-local.sh` + internet |
| [hooks/](hooks/) | Watch signed lifecycle webhook deliveries arrive in real time | `./hack/run-local.sh` |
| [sse-events/](sse-events/) | Stream VM status changes over Server-Sent Events (`GET /events`) | `./hack/run-local.sh` |
| [vm-exec/](vm-exec/) | Create a guest-agent-enabled VM and run a command inside the guest | `./hack/run-local.sh` |
| [vm-commit/](vm-commit/) | Convert an OverlayBD (OCI-backed) VM into a standalone raw-disk VM | `./hack/run-local.sh` |
| [backups/](backups/) | VM and control-plane database backups via `qarax backup` | `./hack/run-local.sh` |
| [network-isolation/](network-isolation/) | VPC routing and isolation between managed subnets, plus VM security groups with live firewall updates | `./hack/run-local.sh` |
| [sandbox/](sandbox/) | Ephemeral VMs from templates with idle-timeout auto-reap and prewarmed pool claims | `./hack/run-local.sh` |
| [firecracker/](firecracker/) | Firecracker backend lifecycle (create/start/pause/resume/stop/delete) | `./hack/run-local.sh` |
| [block-storage/](block-storage/) | Register an iSCSI (LIO) target as a shared `BLOCK` storage pool (API only on the container stack: the node can't log in to the target) | `./hack/run-local.sh` + Docker |
| [gpu-passthrough/](gpu-passthrough/) | GPU passthrough via VFIO to an OCI-booted VM | `./hack/run-local.sh` + VFIO GPU |

## Two-host demos

These need two UP hosts. On the local stack they start the second node
container (`qarax-node-2`, compose profile `multi-node`) and register it as
`local-node-2` themselves.

| Demo | Description |
|------|-------------|
| [network-isolation/](network-isolation/) `--cross-host` | Same-VPC routing across two hosts over the VXLAN overlay, plus live security-group updates |
| [host-evacuation/](host-evacuation/) | Move a VM off a host, leave it in maintenance, and prove new scheduling avoids it |

## Self-contained demos

These bring up their own environment.

| Demo | Description | Requirements |
|------|-------------|--------------|
| [etcd-cluster/](etcd-cluster/) | 3-node etcd cluster, each node as a VM | Docker + KVM + sudo (host veth) |
| [k8s-cluster/](k8s-cluster/) | Upstream 3-node Kubernetes cluster via kubeadm on VMs | Docker + KVM + kubectl, ~10 GiB RAM |
| [hyperconverged/](hyperconverged/) | Control plane running inside a Cloud Hypervisor VM on bare metal (workload VMs default to `passt`) | KVM + podman + root |

## Conventions

Every demo follows these, so re-running one is always safe:

- **Fixed names.** A demo names everything it creates with its own prefix
  (e.g. `demo-hooks-`) and removes leftovers from an interrupted run when it
  starts. No manual cleanup is needed between runs.
- **`KEEP=1`** leaves the demo's resources in place at exit so you can poke
  at them: `KEEP=1 ./demos/vm-exec/run.sh`.
- **`--cleanup`** removes the demo's resources and exits. It never tears down
  the stack; `make stop-local` does that.
- **Assertions.** A demo fails (non-zero exit, red error) when the feature it
  shows didn't happen, rather than printing success regardless.
- `$ qarax ...` lines are the exact commands that ran; with `QARAX_TOKEN`
  exported and the CLI on your PATH you can paste them.

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
- Demos that attach managed networks need the host's `ip_tables` module. If
  `uname -r` has no matching `/lib/modules/` directory (kernel upgraded without
  a reboot), attaching fails with "do you need to insmod?": reboot.

## Writing a new demo

Source the shared helpers (`demos/lib.sh`) instead of redefining them:

```bash
REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
# shellcheck source=demos/lib.sh
source "${REPO_ROOT}/demos/lib.sh"

PREFIX="demo-mything-"
ensure_stack                   # start the local stack if needed, check the token
require_cli                    # build the CLI if needed; `qarax` then targets $SERVER
sweep vm "$PREFIX"             # clear an interrupted run

cleanup() {
	[[ "$KEEP" == 1 ]] && return
	sweep vm "$PREFIX"
}
trap cleanup EXIT

run qarax vm create --name "${PREFIX}vm" --memory 256MiB
run qarax vm start "${PREFIX}vm"   # blocks until the VM is running
```

`lib.sh` also has `ensure_up_hosts N`, `first_up_host`, `start_vms` (parallel
starts), `wait_until`, `wait_vm_status`, `exists`, `wait_guest_agent`, `compose` and
`node_exec`. Commands that are synchronous (create, start, stop, pause,
resume, delete, image import) need no wait afterwards.

## Cleanup

```bash
./demos/<demo>/run.sh --cleanup   # one demo's resources
make stop-local                   # the whole stack and its volumes
```
