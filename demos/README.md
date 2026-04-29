# qarax Demos

Each demo lives in its own directory with a `run.sh` and a `README.md`.

| Demo | Description | Stack required |
|------|-------------|----------------|
| [oci/](oci/) | Boot a VM from an OCI container image via OverlayBD | `./hack/run-local.sh` |
| [boot-source/](boot-source/) | Boot a VM from a kernel + initramfs | `./hack/run-local.sh --with-vm` |
| [hooks/](hooks/) | Watch lifecycle webhook notifications fire in real-time | `make run-local` |
| [etcd-cluster/](etcd-cluster/) | Self-contained 3-node etcd cluster, each node as a VM | Docker + podman + KVM |
| [k8s-cluster/](k8s-cluster/) | Upstream 3-node Kubernetes cluster via kubeadm on VMs | Docker + podman + KVM |
| [gpu-passthrough/](gpu-passthrough/) | GPU passthrough via VFIO to an OCI-booted VM | `make run-local` + VFIO GPU |
| [hyperconverged/](hyperconverged/) | Control plane running inside a Cloud Hypervisor VM on bare metal (defaults to `passt` for workload VMs) | KVM + podman + root |
| [cross-host-vpc/](cross-host-vpc/) | Cross-host same-VPC routing plus live security-group updates across two hosts | two-node e2e stack |
| [host-evacuation/](host-evacuation/) | Manual host evacuation: move a VM off one host, leave it in maintenance, and prove new scheduling avoids it | two-node e2e stack |
| [network-isolation/](network-isolation/) | Same-VPC subnet routing plus VM security groups with live firewall updates | `./hack/run-local.sh` |
| [sandbox/](sandbox/) | Ephemeral VMs from templates with idle-timeout auto-reap and prewarmed pool claims | `./hack/run-local.sh` |
| [firecracker/](firecracker/) | Firecracker backend lifecycle demo (create/start/pause/resume/stop/delete) | `./hack/run-local.sh` |

## Quick start

```bash
# Start the local stack
./hack/run-local.sh

# Run the OCI demo
./demos/oci/run.sh

# Run the hooks demo
./demos/hooks/run.sh

# Run the network isolation demo
./demos/network-isolation/run.sh

# For two-host demos, start the two-node e2e stack first
# (example: cd e2e && KEEP=1 ./run_e2e_tests.sh test_live_migration.py::test_host_evacuation_marks_maintenance_and_avoids_rescheduling)

# Run the cross-host VPC demo
./demos/cross-host-vpc/run.sh

# Run the host evacuation demo
./demos/host-evacuation/run.sh

# Run the Firecracker backend demo
./demos/firecracker/run.sh
```

Networking note:

- `demos/hyperconverged/` now defaults to `passt` for its workload VMs to avoid
  extra bridge/DHCP/NAT setup in the nested environment.
- `demos/etcd-cluster/` and `demos/k8s-cluster/` still use bridged Qarax-managed
  networks because they depend on multi-VM reachability and static guest IPs.

## Cleanup

```bash
# Stop individual VMs
qarax vm stop <name>
qarax vm delete <name>

# Tear down the entire stack
./hack/run-local.sh --cleanup
```
