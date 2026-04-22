# Development Guide

## Host Deploy Testing with libvirt

The script `hack/test-host-deploy-libvirt.sh` creates a libvirt VM from a bootc
container image, registers it as a qarax host, and runs the full deploy flow
(`bootc switch` + reboot).

### Prerequisites

- `podman`, `virsh`, `virt-install`, `qemu-img`, `curl`, `jq`, `nc`, `sshpass`
- libvirt with the `default` network configured
- A running qarax stack (or pass `--start-stack`)

### Quick start (using CI-published image)

```bash
./hack/test-host-deploy-libvirt.sh --keep-vm
```

This uses `ghcr.io/qarax/qarax-vmm-host:latest` as the deploy image, which is
built by CI on every push to `master`.

### Testing with a locally-built deploy image

When iterating on the deploy image (`deployments/Containerfile.qarax-vmm`), you
can test locally using a registry on the libvirt bridge:

```bash
# 1. Build qarax-node
cargo build --release -p qarax-node --target x86_64-unknown-linux-musl

# 2. Build the deploy image
sudo podman build -f deployments/Containerfile.qarax-vmm \
  --build-arg CLOUD_HYPERVISOR_VERSION="$(cat versions/cloud-hypervisor-version)" \
  --build-arg FIRECRACKER_VERSION="$(cat versions/firecracker-version)" \
  -t localhost/qarax-vmm-host:latest .

# 3. Start a local registry (skip if already running)
sudo podman run -d --name qarax-registry -p 5000:5000 docker.io/library/registry:2

# 4. Push to the local registry
sudo podman tag localhost/qarax-vmm-host:latest localhost:5000/qarax-vmm-host:latest
sudo podman push localhost:5000/qarax-vmm-host:latest --tls-verify=false

# 5. Run the test (192.168.122.1 is the libvirt default bridge gateway)
DEPLOY_IMAGE=192.168.122.1:5000/qarax-vmm-host:latest \
  ./hack/test-host-deploy-libvirt.sh --keep-vm
```

> [!NOTE]
> The test VM is pre-configured to trust `192.168.122.1:5000` as an insecure
> (HTTP) registry. If you need a different registry address, update the
> `registries.conf.d` entry in the test host Containerfile inside the script.

### Rebuilding the test VM base image

The test VM disk image is cached at `/tmp/qarax-libvirt-deploy/base-bootc.qcow2`.
Delete it to force a rebuild (e.g. after changing the test host Containerfile):

```bash
rm -f /tmp/qarax-libvirt-deploy/base-bootc.qcow2
```

### Cleanup

```bash
./hack/test-host-deploy-libvirt.sh --cleanup
```

### Script options

| Flag             | Description                                    |
|------------------|------------------------------------------------|
| `--keep-vm`      | Preserve the VM after the test (pass or fail)  |
| `--start-stack`  | Start the qarax docker-compose stack if needed |
| `--cleanup`      | Remove the VM and exit                         |

### Environment variables

| Variable          | Default                                         | Description                           |
|-------------------|-------------------------------------------------|---------------------------------------|
| `DEPLOY_IMAGE`    | `ghcr.io/qarax/qarax-vmm-host:latest`           | Image used for `bootc switch`         |
| `VM_BOOTC_IMAGE`  | `quay.io/centos-bootc/centos-bootc:stream10`    | Base image for the test VM            |
| `API_URL`         | `http://localhost:8000`                          | qarax API endpoint                    |
| `VM_MEMORY_MB`    | `3072`                                           | VM memory                             |
| `VM_VCPUS`        | `2`                                              | VM vCPUs                              |
| `VM_DISK_SIZE`    | `20G`                                            | VM disk size                          |
| `SSH_USER`        | `qarax`                                          | Test VM SSH user                      |
| `SSH_PASSWORD`    | `qarax`                                          | Test VM SSH password                  |
