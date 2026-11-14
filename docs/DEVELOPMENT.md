# Development Guide

## The bootc node microVM

`make run-local VM=1` and the e2e node-upgrade tests run qarax-node the way a
real host does: the appliance image (`deployments/Containerfile.qarax-vmm`),
installed with bootc and booted as a Cloud Hypervisor microVM. The compose
service `bootc-vm` (profile `bootc`) is that VM; the rest of the stack reaches
its SSH and gRPC on the service's address, like any other node.

### Prerequisites

- The local Docker stack's requirements (Docker with Compose, `/dev/kvm`, a
  Rust toolchain)
- `qemu-img`
- Nested virtualization on the host (`cat /sys/module/kvm_intel/parameters/nested`
  or `kvm_amd`), so the node VM can run VMs of its own
- A kernel whose modules are loadable (after a kernel upgrade, reboot): the
  disk is installed through a loop device

No sudo, podman or bootc-image-builder: the install runs in a privileged
container.

### Quick start

```bash
make run-local VM=1          # stack + node VM, registered as host local-node-vm
qarax vm create --name alpine --image-ref public.ecr.aws/docker/library/alpine:latest
qarax vm start alpine        # boots nested inside the node VM

cd e2e && ENABLE_BOOTC_UPGRADE_E2E=1 ./run_e2e_tests.sh test_node_upgrade.py
```

### How it's built

`e2e/build-bootc-node.sh` (run by both of the above):

1. Builds the appliance image as `localhost:5001/qarax-vmm-host:e2e` from the
   release binaries.
2. Adds the test layer (`e2e/Containerfile.bootc-node`): root SSH with password
   `testpassword`, the registry trusted over HTTP, and a boot unit that names
   the VM's gateway `registry`. It pushes the result as
   `localhost:5001/qarax-node-bootc:base`.
3. Installs that image to `e2e/bootc-vm-base.qcow2` with
   `bootc install to-disk --via-loopback`, run from the image itself in a
   privileged container. The disk is rebuilt only when the image ID changes
   (`bootc-vm-base.qcow2.image-id`).
4. Creates a fresh `e2e/bootc-vm-overlay.qcow2` over it, so every boot starts
   from the installed state.

`e2e/Dockerfile.bootc-vm` boots the overlay with UEFI firmware and nested
virtualization. Inside its container the guest is `10.0.2.15` on a TAP device,
the container is its gateway `10.0.2.2` (DHCP from dnsmasq, NAT out), port
5000 there proxies the compose registry, and the container's ports 22 and
50051 are forwarded to the guest.

Because the guest resolves `registry` to its gateway, the stack's OverlayBD
pool (`http://registry:5000`) works on the VM node unchanged, and
`bootc switch` pulls from `10.0.2.2:5000/...` (the upgrade tests' images).

### What VM mode runs

OCI-image VMs (OverlayBD) and disk-image VMs (UEFI firmware): the appliance
ships the guest kernel and `CLOUDHV.fd`. VMs that boot the default kernel with
the e2e test initramfs need the container node, which is the only one that has
that initramfs.

In VM mode `run-local` stops the `qarax-node` container if it's running and
marks `local-node` down, so new VMs land on `local-node-vm`.

### Getting into the node

```bash
docker compose -f e2e/docker-compose.yml logs -f bootc-vm      # serial console
docker compose -f e2e/docker-compose.yml exec bootc-vm \
  ssh root@10.0.2.15                                         # password: testpassword
```

`journalctl -u qarax-node` in the guest shows the node's log.

### Troubleshooting

- `losetup: ... failed to set up loop device`: the `loop` module can't load,
  usually because the kernel was upgraded without a reboot.
- `bootc-vm` never gets healthy: read its serial console (`logs bootc-vm`);
  the guest should reach `qarax-node.service` within about 15 s.
- A deploy fails with `Unknown server key`: the qarax container pinned the SSH
  host key of an earlier disk at the same address. A disk rebuild generates
  new keys; `make stop-local` (or recreating the qarax container) clears the
  pin.
- Workload VMs fail with `/dev/kvm` missing in the node: the host doesn't
  allow nested virtualization.
