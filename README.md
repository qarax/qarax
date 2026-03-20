# qarax

## Description

qarax is a management platform for managing virtual machines with Cloud Hypervisor.

## Architecture

qarax consists of two main components:

- **qarax** (this repository): Control plane REST API server that manages VM and host lifecycle
- **qarax-node**: Data plane gRPC service that runs on hypervisor hosts and manages VM execution via Cloud Hypervisor

## Building

```bash
# Build all packages and generate OpenAPI spec
make build

# Generate OpenAPI spec only
make openapi

# Run tests
make test

# Or use cargo directly (won't auto-generate OpenAPI)
cargo build
```

The project includes auto-generated OpenAPI 3.1 documentation. Access it at http://localhost:8000/swagger-ui when the server is running.

## Run locally (Docker stack)

To run the full stack (qarax + qarax-node + PostgreSQL) in Docker for local testing:

```bash
./hack/run-local.sh
```

Requires Docker, Docker Compose, KVM (`/dev/kvm`), and a Rust toolchain. The script builds qarax-node, starts all services, and prints the API and Swagger UI URLs. Stop with `cd e2e && docker compose down -v`.

## VM Boot Configuration

qarax uses configurable default boot artifacts for VMs. Configure these in your environment's YAML file:

```yaml
vm_defaults:
  kernel: "/var/lib/qarax/images/vmlinux"
  initramfs: "/var/lib/qarax/images/initramfs.gz"
  cmdline: "console=ttyS0 console=hvc0 root=/dev/vda1"
```

### Using Test Artifacts (E2E/Local Development)

For E2E tests and local development, the default configuration uses test artifacts that boot and shut down after 5 seconds. These are useful for verifying VM creation but not for running persistent VMs.

### Using Production Images

For production VMs that stay running, replace the test artifacts with proper bootable images:

1. **Option 1: Cloud Images** (Recommended)
   ```bash
   # Download a cloud image (e.g., Ubuntu)
   wget https://cloud-images.ubuntu.com/minimal/releases/jammy/release/ubuntu-22.04-minimal-cloudimg-amd64.img

   # Extract kernel and initrd from the image
   virt-get-kernel ubuntu-22.04-minimal-cloudimg-amd64.img

   # Update configuration to point to these files
   ```

2. **Option 2: Custom Build**
   Build your own kernel and initramfs with the tools and init system you need.

Update your `configuration/production.yaml`:
```yaml
vm_defaults:
  kernel: "/var/lib/qarax/images/vmlinux-production"
  initramfs: "/var/lib/qarax/images/initramfs-production.gz"
  cmdline: "console=ttyS0 console=hvc0 root=/dev/vda1 init=/sbin/init"
```

## Provisioning a VM (API walkthrough)

The full interactive schema is available at `http://localhost:8000/swagger-ui` when the server is running.

### Scheduling

When creating a VM, qarax picks a host that is in `up` state (i.e. with a reachable qarax-node). Subsequent operations (start, stop, pause, resume, delete) are routed to whichever host the VM was scheduled on, stored as `host_id` on the VM record.

Register hosts via `POST /hosts` and set their status to `up` via `PATCH /hosts/{host_id}` to make them eligible for scheduling. If no `up` host exists, VM creation returns a 422.

### Step 1 — Register a storage pool

Storage pools group the directories where images live on the hypervisor hosts.

**Supported pool types:** `local`, `nfs`

- **local**: `config.path` is a directory on the host (e.g. `/var/lib/qarax/images`). Place kernel, initramfs, and disk files there before registering them.
- **nfs**: `config.path` is the NFS mount point on the host. Mount the NFS share on each hypervisor host, then use paths under that mount when registering objects.

**How to get files into the pool:**

- **Local pool**: Copy files (e.g. kernel, initramfs) to the host path before creating storage objects. Example:
  ```bash
  scp vmlinux initramfs.gz root@hypervisor-host:/var/lib/qarax/images/
  ```
- **NFS pool**: Copy files to the NFS export, or ensure they exist at the paths you will register. Each hypervisor host must have the NFS share mounted at the same path.

```bash
curl -s -X POST http://localhost:8000/storage-pools \
  -H 'Content-Type: application/json' \
  -d '{
    "name": "local-images",
    "pool_type": "local",
    "config": {"path": "/var/lib/qarax/images"}
  }'
# Returns: <pool-uuid>
```

### Step 2 — Register storage objects (kernel + initramfs)

Each object points to a real file on the host via `config.path`. This path is what gets passed to Cloud Hypervisor at boot time. Ensure the file exists at that path on the hypervisor host where the VM will run.

```bash
# Kernel
curl -s -X POST http://localhost:8000/storage-objects \
  -H 'Content-Type: application/json' \
  -d '{
    "name": "vmlinux-6.1",
    "storage_pool_id": "<pool-uuid>",
    "object_type": "kernel",
    "size_bytes": 0,
    "config": {"path": "/var/lib/qarax/images/vmlinux"}
  }'
# Returns: <kernel-uuid>

# Initramfs (optional)
curl -s -X POST http://localhost:8000/storage-objects \
  -H 'Content-Type: application/json' \
  -d '{
    "name": "test-initramfs",
    "storage_pool_id": "<pool-uuid>",
    "object_type": "initrd",
    "size_bytes": 0,
    "config": {"path": "/var/lib/qarax/images/initramfs.gz"}
  }'
# Returns: <initrd-uuid>
```

`object_type` values: `disk`, `kernel`, `initrd`, `iso`, `snapshot`

### Step 3 — Create a boot source

A boot source links a kernel and optional initramfs, and sets the kernel command line.

```bash
curl -s -X POST http://localhost:8000/boot-sources \
  -H 'Content-Type: application/json' \
  -d '{
    "name": "linux-6.1",
    "kernel_image_id": "<kernel-uuid>",
    "initrd_image_id": "<initrd-uuid>",
    "kernel_params": "console=ttyS0 reboot=k panic=1 nomodules"
  }'
# Returns: <boot-source-uuid>
```

`initrd_image_id` and `kernel_params` are optional. If you omit `boot_source_id` when creating a VM, the server falls back to `vm_defaults` from the yaml config.

### Step 3.5 — Optional: create instance types and VM templates

Use **instance types** for reusable sizing presets and **VM templates** for reusable VM blueprints. In practice, templates are most useful when they define an `image_ref`, a `boot_source_id` plus `root_disk_object_id`, or are created from an existing VM.

```bash
# Instance type
curl -s -X POST http://localhost:8000/instance-types \
  -H 'Content-Type: application/json' \
  -d '{
    "name": "gpu-small",
    "boot_vcpus": 4,
    "max_vcpus": 8,
    "memory_size": 1073741824
  }'
# Returns: <instance-type-uuid>

# VM template with an OCI root image
curl -s -X POST http://localhost:8000/vm-templates \
  -H 'Content-Type: application/json' \
  -d '{
    "name": "ubuntu-ai-base",
    "hypervisor": "cloud_hv",
    "image_ref": "docker.io/library/ubuntu:22.04",
    "config": {"profile": "ai"}
  }'
# Returns: <vm-template-uuid>

# VM template with a storage-pool-backed root disk
curl -s -X POST http://localhost:8000/vm-templates \
  -H 'Content-Type: application/json' \
  -d '{
    "name": "ubuntu-disk-base",
    "hypervisor": "cloud_hv",
    "boot_mode": "firmware",
    "boot_vcpus": 2,
    "max_vcpus": 2,
    "memory_size": 536870912,
    "root_disk_object_id": "<storage-object-uuid>"
  }'
# Returns: <vm-template-uuid>
```

You can also create a template from an existing VM:

```bash
curl -s -X POST http://localhost:8000/vms/<vm-uuid>/template \
  -H 'Content-Type: application/json' \
  -d '{
    "name": "ubuntu-from-vm"
  }'
# Returns: <vm-template-uuid>
```

This copies reusable VM-create fields such as compute, memory, boot settings, `image_ref`, `root_disk_object_id`, cloud-init data, and config. It intentionally does **not** copy host-specific runtime details like TAP device names or concrete allocated guest IPs.

When creating a VM, field precedence is:

1. Fields supplied directly in `POST /vms`
2. The selected `instance_type_id` for sizing-related fields
3. The selected `vm_template_id` for reusable VM defaults
4. Server-side `vm_defaults` as the final fallback for boot artifacts

### Step 4 — Create the VM

This call registers the VM in the database and calls qarax-node via gRPC to create the Cloud Hypervisor instance. Network interfaces are attached here.

**Minimal (no networking):**

```bash
curl -s -X POST http://localhost:8000/vms \
  -H 'Content-Type: application/json' \
  -d '{
    "name": "my-vm",
    "tags": ["dev", "ci"],
    "hypervisor": "cloud_hv",
    "boot_vcpus": 2,
    "max_vcpus": 2,
    "memory_size": 536870912,
    "boot_source_id": "<boot-source-uuid>"
  }'
# Returns: <vm-uuid>
```

`memory_size` is in bytes (536870912 = 512 MiB).
`tags` is optional and accepts a list of strings for grouping or ownership metadata.

**Using a template + instance type:**

```bash
curl -s -X POST http://localhost:8000/vms \
  -H 'Content-Type: application/json' \
  -d '{
    "name": "my-ai-vm",
    "vm_template_id": "<vm-template-uuid>",
    "instance_type_id": "<instance-type-uuid>",
    "boot_vcpus": 16,
    "config": {"owner": "ml-team"}
  }'
```

**Using a boot source plus a storage-backed root disk directly:**

```bash
curl -s -X POST http://localhost:8000/vms \
  -H 'Content-Type: application/json' \
  -d '{
    "name": "ubuntu-disk-vm",
    "hypervisor": "cloud_hv",
    "boot_vcpus": 2,
    "max_vcpus": 2,
    "memory_size": 536870912,
    "boot_source_id": "<boot-source-uuid>",
    "root_disk_object_id": "<storage-object-uuid>"
  }'
```

**Creating a template from a VM and reusing it:**

```bash
# Create the reusable template
curl -s -X POST http://localhost:8000/vms/<existing-vm-uuid>/template \
  -H 'Content-Type: application/json' \
  -d '{"name": "golden-ubuntu"}'

# Launch a new VM from it
curl -s -X POST http://localhost:8000/vms \
  -H 'Content-Type: application/json' \
  -d '{
    "name": "golden-ubuntu-copy",
    "vm_template_id": "<vm-template-uuid>"
  }'
```

**With a TAP network interface:**

The TAP device must already exist on the host before calling this. Each entry in `networks` requires an `id` — the virtio device name visible inside the guest (e.g. `"net0"`).

```bash
curl -s -X POST http://localhost:8000/vms \
  -H 'Content-Type: application/json' \
  -d '{
    "name": "my-vm",
    "hypervisor": "cloud_hv",
    "boot_vcpus": 2,
    "max_vcpus": 2,
    "memory_size": 536870912,
    "boot_source_id": "<boot-source-uuid>",
    "networks": [
      {
        "id": "net0",
        "tap": "tap0",
        "mac": "52:54:00:12:34:56",
        "ip": "192.168.100.10",
        "mask": "255.255.255.0",
        "mtu": 1500
      }
    ]
  }'
```

Interface type is inferred from the fields: `vhost_user: true` → vhost-user, `tap` present → TAP, neither → MACVTAP.

Multiple interfaces are supported — add more objects to the `networks` array with distinct `id` values (`"net0"`, `"net1"`, etc.).

**Root disk sources:**

- `image_ref` uses the OCI image workflow (OverlayBD on hosts that support it, otherwise virtiofs).
- `root_disk_object_id` uses an existing storage object from a storage pool and persists it as the VM's boot disk in `vm_disks`.
- `VM_ROOTFS` remains as a legacy server-side fallback, but first-class `root_disk_object_id` is preferred for normal API/CLI use.

### Step 5 — Start the VM

```bash
curl -s -X POST http://localhost:8000/vms/<vm-uuid>/start
```

### Step 6 — Check status

```bash
curl -s http://localhost:8000/vms/<vm-uuid> | jq .
```

VM `status` values: `unknown`, `created`, `running`, `paused`, `shutdown`

### Other lifecycle operations

```bash
curl -s -X POST   http://localhost:8000/vms/<vm-uuid>/pause
curl -s -X POST   http://localhost:8000/vms/<vm-uuid>/resume
curl -s -X POST   http://localhost:8000/vms/<vm-uuid>/stop
curl -s -X DELETE http://localhost:8000/vms/<vm-uuid>
```

Serial console output is written to `/var/lib/qarax/vms/<vm-uuid>.console.log` on the host running qarax-node.

## Host Provisioning

qarax uses bootc (bootable containers) to deploy VMM (Virtual Machine Manager) hosts. The bootc image includes qarax-node, Cloud Hypervisor, and all necessary dependencies.

### Host Requirements

Before deployment, provide a pre-installed Linux host that:

- is reachable via SSH from the qarax control plane
- has virtualization support enabled (`/dev/kvm`)
- can expose qarax-node on a reachable gRPC port (default `50051`)

### Development Mode (Direct Deployment)

During development, use direct mode to quickly deploy qarax-node binaries:

```bash
# Build qarax-node
cargo build -p qarax-node

# Deploy to test host
scp target/debug/qarax-node root@192.168.1.100:/usr/local/bin/qarax-node
ssh root@192.168.1.100 "systemctl restart qarax-node"
```

# bootc setup

```bash
# Build release binary
cargo build --release -p qarax-node

# Build bootc image
podman build -f deployments/Containerfile.qarax-vmm -t quay.io/yourorg/qarax-vmm-host:v1.0.0 .
podman push quay.io/yourorg/qarax-vmm-host:v1.0.0

# Register the host first (port is qarax-node gRPC port)
curl -X POST http://qarax:8000/hosts \
  -H 'Content-Type: application/json' \
  -d '{
    "name": "vmm-host-1",
    "address": "10.0.0.42",
    "port": 50051,
    "host_user": "root",
    "password": ""
  }'

# Deploy bootc image over SSH
curl -X POST http://qarax:8000/hosts/{host_id}/deploy \
  -H 'Content-Type: application/json' \
  -d '{
    "image": "quay.io/yourorg/qarax-vmm-host:v1.0.0",
    "ssh_port": 22,
    "ssh_user": "root",
    "ssh_private_key_path": "/home/qarax/.ssh/id_ed25519",
    "install_bootc": true,
    "reboot": true
  }'
```
