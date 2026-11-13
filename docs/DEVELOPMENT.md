# Development Guide

## Host Deploy Testing with libvirt

The script `hack/test-host-deploy-libvirt.sh` runs qarax-node in a libvirt VM
and registers it with the local control plane through the real host-deploy
flow: `qarax host deploy` SSHes into the VM, runs `bootc switch` to the node
image and reboots it, and the host comes back `up`. `make run-local VM=1`
(`hack/run-local.sh --vm`) runs it with `--keep-vm` to put the local stack's
node in a VM instead of a container.

### Prerequisites

- libvirt + QEMU (`virsh`, `virt-install`, `qemu-img`) with the `default` NAT
  network; your user must be able to use `qemu:///system` (usually the
  `libvirt` group)
- rootful podman through `sudo`, for the image builds and bootc-image-builder
- Docker with Compose, `jq`, `curl`, `ssh-keygen`
- Release binaries: `cargo build --release -p qarax -p qarax-node -p qarax-init -p cli`
  (`make run-local` and `--start-stack` build them)

### Quick start

```bash
# Full deploy test on a new VM, destroyed on exit
./hack/test-host-deploy-libvirt.sh --start-stack

# Leave the node running (what make run-local VM=1 does)
./hack/test-host-deploy-libvirt.sh --keep-vm
```

### What it does

1. **Stack.** Applies `e2e/docker-compose.vm-mode.yml` to registry, postgres
   and qarax: qarax runs on host networking, because libvirt's NAT network only
   accepts connections from the host itself. Compose recreates only what
   changed, so this is a no-op after `make run-local VM=1`. With
   `--start-stack` and no stack reachable it builds missing release binaries
   and starts those three services.
2. **Images.** Builds the node appliance `deployments/Containerfile.qarax-vmm`
   as `localhost/qarax-vmm-host:dev` with rootful podman (the build args of
   `make appliance-build`), then a thin test layer on top of it,
   `localhost/qarax-test-host:latest`. The test layer's Containerfile is
   generated into `$STATE_DIR/build/` and adds only what the test host needs:
   a password SSH login with passwordless sudo for `SSH_USER`, a fixed SSH host
   key, and the compose registry (`<libvirt bridge IP>:5001`) as an insecure
   registry. Images these builds replace are removed by ID.
3. **VM.** Boots from a qcow2 that bootc-image-builder makes from the test
   image, with a copy-on-write overlay disk per VM. The VM has a fixed MAC, so
   a recreated VM gets the same DHCP address.
4. **Deploy.** Registers the VM as host `libvirt-node` (once; re-runs reuse
   the record), pushes the test image to the registry as
   `<bridge IP>:5001/qarax-test-host:<image ID>`, runs `qarax host deploy`
   and waits for the host to come back `up`, then `qarax host init`.

### Re-runs and `--keep-vm`

With `--keep-vm` the VM stays running, and the next `--keep-vm` run reuses it:

- If the VM already runs the image this run built (nothing changed), the run
  skips the deploy: it registers `libvirt-node` again if the database was
  reset and runs `qarax host init`. This takes seconds.
- Otherwise it redeploys the new image onto the running VM (`bootc switch` +
  reboot).
- `--fresh` destroys and recreates the VM, then deploys.

Without `--keep-vm` every run is a full test: it replaces any VM a
`--keep-vm` run left (same `VM_NAME`), deploys, and on exit destroys the VM
and deletes the `libvirt-node` host record.

### Cache

`STATE_DIR` (default `/var/lib/libvirt/images/qarax-dev`) holds the base disk,
the VM's overlay disk, the generated test layer with its SSH host key, and the
name of the image the VM runs. It is on disk rather than in `/tmp`, which is
often tmpfs, and under libvirt's image directory, which the `qemu:///system`
emulator user can reach and which has the right SELinux/AppArmor labels. The
script creates it owned by you.

The base disk is cached as `base-<key>.qcow2`, keyed by the appliance
Containerfile, `versions/node-artifacts.env`, the Cloud Hypervisor and
Firecracker versions and the test layer, but not by the binaries: the deploy
installs the current binaries over whatever the base booted with. A recipe
change rebuilds the base the next time a VM is created. `/var` is the one
thing `bootc switch` leaves alone, so content the appliance puts under `/var`
reaches a VM only through its base disk: use `--fresh` after changing it.

### Cleanup

```bash
./hack/test-host-deploy-libvirt.sh --cleanup
```

This destroys the VM and its overlay disk and deletes the `libvirt-node` host
record. Base disks stay cached; `sudo rm -rf
/var/lib/libvirt/images/qarax-dev` removes everything. Pushed test images stay
in the compose registry's volume until `make stop-local`.

### Troubleshooting

- Console: `virsh -c qemu:///system console qarax-deploy-test`
- SSH: `ssh qarax@<VM IP>` (password `qarax`), then `sudo journalctl -u qarax-node`
- Deploy errors: `qarax host get libvirt-node` shows the failure;
  `docker compose -f e2e/docker-compose.yml logs qarax` has the details
- "host libvirt-node is registered at A, but VM is at B": the VM got a new
  DHCP address. qarax can't change a host's address, so reset the stack with
  `make stop-local` and run again.

### Script options

| Flag            | Description                                                    |
|-----------------|----------------------------------------------------------------|
| `--keep-vm`     | Leave the VM running, and reuse a kept VM                      |
| `--fresh`       | With `--keep-vm`: recreate the VM instead of reusing it        |
| `--start-stack` | Start registry, postgres and qarax if the API isn't reachable  |
| `--cleanup`     | Destroy the VM, delete its host record, and exit               |

### Environment variables

| Variable              | Default                                       | Description                                  |
|-----------------------|-----------------------------------------------|----------------------------------------------|
| `API_URL`             | `http://localhost:8000`                       | qarax API endpoint                           |
| `QARAX_TOKEN`         | `e2e-test-token`                              | API token                                    |
| `VM_NAME`             | `qarax-deploy-test`                           | libvirt domain name                          |
| `VM_MEMORY_MB`        | `3072`                                        | VM memory                                    |
| `VM_VCPUS`            | `2`                                           | VM vCPUs                                     |
| `VM_DISK_SIZE`        | `20G`                                         | VM overlay disk size                         |
| `VM_MAC`              | `52:54:00:71:a8:01`                           | VM MAC address                               |
| `HOST_NAME`           | `libvirt-node`                                | qarax host name                              |
| `SSH_USER`            | `qarax`                                       | Test VM SSH user                             |
| `SSH_PASSWORD`        | `qarax`                                       | Test VM SSH password                         |
| `LIBVIRT_BRIDGE_IP`   | address of the libvirt `default` network      | Address the VM reaches the registry on       |
| `LOCAL_REGISTRY_PORT` | `5001`                                        | Compose registry's published port            |
| `STATE_DIR`           | `/var/lib/libvirt/images/qarax-dev`           | Disk and build cache                         |
| `BIB_IMAGE`           | `quay.io/centos-bootc/bootc-image-builder@sha256:…` | bootc-image-builder image (pinned digest) |
