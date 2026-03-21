# qarax Demos

Each demo lives in its own directory with a `run.sh` and a `README.md`.

| Demo | Description | Stack required |
|------|-------------|----------------|
| [oci/](oci/) | Boot a VM from an OCI container image via OverlayBD | `./hack/run-local.sh` |
| [boot-source/](boot-source/) | Boot a VM from a kernel + initramfs | `./hack/run-local.sh --with-vm` |
| [hooks/](hooks/) | Watch lifecycle webhook notifications fire in real-time | `make run-local` |
| [etcd-cluster/](etcd-cluster/) | Self-contained 3-node etcd cluster, each node as a VM | Docker + podman + KVM |
| [gpu-passthrough/](gpu-passthrough/) | GPU passthrough via VFIO to an OCI-booted VM | `make run-local` + VFIO GPU |
| [hyperconverged/](hyperconverged/) | Control plane running inside a Cloud Hypervisor VM on bare metal | KVM + podman + root |

## Quick start

```bash
# Start the local stack
./hack/run-local.sh

# Run the OCI demo
./demos/oci/run.sh

# Run the hooks demo
./demos/hooks/run.sh
```

## Cleanup

```bash
# Stop individual VMs
qarax vm stop <name>
qarax vm delete <name>

# Tear down the entire stack
./hack/run-local.sh --cleanup
```
