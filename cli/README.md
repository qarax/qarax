# qarax CLI

Command-line interface for the qarax VM management API.

## Install

```bash
cargo build -p cli --release
# binary: target/release/qarax
```

## Configure

```bash
qarax configure --server http://192.168.1.10:8000
```

The server URL is resolved in this order:

1. `--server` flag or `QARAX_SERVER` env var
2. `~/.config/qarax/config.toml`
3. `http://localhost:8000`

Use `-o json` or `-o yaml` to change output format.

## Names or IDs

All commands accept resource names or UUIDs interchangeably:

```bash
qarax vm start my-vm
qarax vm start 3f6c2b1a-0000-0000-0000-000000000001
```

## Commands

| Command | Description |
|---|---|
| `qarax vm` | Virtual machine operations |
| `qarax host` | Hypervisor host operations |
| `qarax storage-pool` | Storage pool operations |
| `qarax storage-object` | Storage object operations |
| `qarax boot-source` | Boot source operations |
| `qarax network` | Network operations |
| `qarax instance-type` | Instance type operations |
| `qarax vm-template` | VM template operations |
| `qarax hook` | Lifecycle webhook operations |
| `qarax transfer` | File transfer operations |
| `qarax job` | Async job status |
| `qarax sandbox` | Ephemeral sandbox VMs |
| `qarax audit-log` | Audit log inspection |

Run `qarax <command> --help` for full usage of any command.

### Sandbox pools

Keep standby sandboxes ready for a template so `sandbox create` can claim one instantly:

```bash
qarax sandbox pool set --template ubuntu-base --min-ready 1
qarax sandbox pool get --template ubuntu-base
qarax sandbox pool list
qarax sandbox create --template ubuntu-base --wait
qarax sandbox pool delete --template ubuntu-base
```

Warm claims currently apply to plain template-based sandbox requests; if you pass extra create-time overrides such as `--network`, qarax falls back to the cold provisioning path.

## Provisioning a VM

### Scheduling

When creating a VM, qarax picks a host in `up` state. Hosts in `maintenance` stay manageable but are excluded from new scheduling. Subsequent operations route to whichever host the VM was scheduled on. Add and initialize a host first to make scheduling work.

### Hosts

```bash
# Add a host (password auth)
qarax host add --name node-01 --address 192.168.1.10 --user root --password secret

# Add a host (SSH key auth, no password needed)
qarax host add --name node-01 --address 192.168.1.10 --user root

# Initialize (connects via gRPC, marks it UP)
qarax host init node-01

# Deploy a bootc image
qarax host deploy node-01 --image ghcr.io/example/qarax-node:latest --ssh-key ~/.ssh/id_ed25519

# Keep a host out of new placement
qarax host maintenance enter node-01
qarax host maintenance exit node-01

# Live-evacuate running/paused VMs and leave the host in maintenance
qarax host evacuate node-01

# Inspect
qarax host list
qarax host get node-01
qarax host gpus node-01
```

### Storage pools

Storage pools group directories where images live on hypervisor hosts.

Pool types: `local`, `nfs`, `overlaybd`

```bash
# Local pool (host-specific, must specify --host)
qarax storage-pool create --name local-images --pool-type local \
  --path /var/lib/qarax/local-images --host node-01

# Shared pools (nfs, overlaybd) auto-attach all UP hosts
qarax storage-pool create --name shared-pool --pool-type nfs \
  --config '{"server":"10.0.0.5","path":"/export/vms"}'

qarax storage-pool create --name obd-pool --pool-type overlaybd \
  --config '{"url":"http://registry:5000"}'
```

### Storage objects and transfers

Each object points to a file in a pool. Transfers download remote files into a pool.

```bash
# Transfer a kernel
qarax transfer create --pool local-images --name vmlinux \
  --source https://example.com/vmlinux --object-type kernel

# Transfer an initramfs
qarax transfer create --pool local-images --name test-initramfs \
  --source https://example.com/initramfs.gz --object-type initrd

# Or create a storage object directly (file must already exist on the host)
qarax storage-object create --name my-disk --pool local-images --object-type disk --size 10737418240
```

Object types: `disk`, `kernel`, `initrd`, `iso`, `snapshot`, `oci_image`

### Boot sources

A boot source links a kernel, optional initramfs, and kernel command line.

```bash
qarax boot-source create --name linux-6.1 --kernel vmlinux \
  --initrd test-initramfs --params "console=ttyS0 reboot=k panic=1 nomodules"
```

If you omit `--boot-source` when creating a VM, the server falls back to `vm_defaults` from the YAML config.

### Instance types and VM templates

Instance types provide reusable sizing presets:

```bash
qarax instance-type create --name gpu-small --vcpus 4 --max-vcpus 8 --memory 1073741824
```

VM templates provide reusable VM blueprints:

```bash
# From an OCI image
qarax vm-template create --name ubuntu-base --image-ref docker.io/library/ubuntu:22.04

# From an existing VM
qarax vm template create my-vm --name golden-ubuntu
```

When creating a VM, field precedence is:
1. Fields supplied directly in `qarax vm create`
2. The selected `--instance-type` for sizing
3. The selected `--template` for defaults
4. Server-side `vm_defaults` as fallback

### Creating VMs

```bash
# Minimal
qarax vm create --name my-vm --vcpus 2 --memory 536870912 --boot-source linux-6.1

# With a network
qarax vm create --name my-vm --vcpus 2 --memory 536870912 \
  --boot-source linux-6.1 --network my-network

# With a static IP
qarax vm create --name my-vm --vcpus 2 --memory 536870912 \
  --boot-source linux-6.1 --network my-network --ip 192.168.100.10

# With a storage-backed root disk
qarax vm create --name my-vm --vcpus 2 --memory 536870912 \
  --boot-source linux-6.1 --root-disk my-disk-object

# Disk-backed cloud image (UEFI/firmware boot)
qarax vm create --name jammy-vm --vcpus 2 --memory 1073741824 \
  --boot-mode firmware --root-disk ubuntu-22.04-cloud \
  --network lan-vms --cloud-init-user-data ./user-data.yaml

# OCI image (async, polls until ready)
qarax vm create --name my-oci-vm --vcpus 2 --memory 536870912 \
  --image-ref public.ecr.aws/docker/library/ubuntu:22.04

# With cloud-init
qarax vm create --name my-vm --vcpus 2 --memory 536870912 \
  --boot-source linux-6.1 --network my-network --cloud-init-user-data ./user-data.yaml

# Using a template + instance type
qarax vm create --name my-ai-vm --template ubuntu-base --instance-type gpu-small

# With tags
qarax vm create --name my-vm --vcpus 2 --memory 536870912 \
  --boot-source linux-6.1 --tag dev --tag ci
```

`--memory` is in bytes (536870912 = 512 MiB).

### VM lifecycle

```bash
qarax vm start my-vm
qarax vm stop my-vm
qarax vm force-stop my-vm
qarax vm pause my-vm
qarax vm resume my-vm
qarax vm delete my-vm

# Console
qarax vm console my-vm      # print stored serial log
qarax vm attach my-vm       # interactive WebSocket console (Ctrl-C to exit)
```

### Audit logs

```bash
qarax audit-log list --resource-type vm --action create
qarax audit-log get 3f6c2b1a-0000-0000-0000-000000000001
```

### Disk and NIC hotplug

```bash
qarax vm attach-disk my-vm --object my-disk-object
qarax vm remove-disk my-vm --device-id disk0
qarax vm add-nic my-vm --network my-network
qarax vm remove-nic my-vm --device-id net1
```

### Live resize

```bash
qarax vm resize my-vm --vcpus 4
qarax vm resize my-vm --ram 1073741824
```

### Snapshots

```bash
qarax vm snapshot create my-vm --name snap-1
qarax vm snapshot list my-vm
qarax vm snapshot restore my-vm --snapshot snap-1
```

### Live migration

```bash
qarax vm migrate my-vm --host node-02
```

Requires shared storage (NFS or OverlayBD) between both hosts.

## Lifecycle hooks

```bash
# Global hook
qarax hook create --name notify-all \
  --url https://hooks.example.com/qarax --secret my-hmac-secret

# VM-scoped hook
qarax hook create --name notify-my-vm \
  --url https://hooks.example.com/qarax \
  --scope vm --scope-value <vm-uuid> --events vm.started,vm.stopped

# Tag-scoped hook
qarax hook create --name notify-prod \
  --url https://hooks.example.com/qarax --scope tag --scope-value prod

# Inspect executions
qarax hook executions notify-all
```

## Jobs

Long-running operations (like OCI image pulls) run as async jobs.
`vm create` and `vm start` poll automatically, but you can check manually:

```bash
qarax job get <job-uuid>
```

## End-to-end example

```bash
qarax configure --server http://localhost:8000

# 1. Add and initialize a host
qarax host add --name node-01 --address 192.168.1.10 --user root --password secret
qarax host init node-01

# 2. Create a local storage pool on the host
qarax storage-pool create --name local-pool --pool-type local \
  --path /var/lib/qarax/local-pool --host node-01

# 3. Transfer a kernel and initrd
qarax transfer create --pool local-pool --name vmlinux \
  --source https://cloud-hypervisor.azureedge.net/vmlinux --object-type kernel
qarax transfer create --pool local-pool --name test-initramfs \
  --source https://example.com/test-initramfs.gz --object-type initrd

# 4. Create a boot source
qarax boot-source create --name my-bs --kernel vmlinux \
  --initrd test-initramfs --params "console=ttyS0 reboot=k panic=1 pci=off"

# 5. Create and start a VM
qarax vm create --name my-vm --vcpus 2 --memory 1073741824 --boot-source my-bs
qarax vm start my-vm

# 6. Attach console
qarax vm attach my-vm
```
