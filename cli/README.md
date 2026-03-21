# qarax (CLI)

Command-line interface for the [qarax](../) VM management API.

## Installation

```bash
cargo build -p cli --release
# Binary will be at: target/release/qarax
```

## Configuration

Run `qarax configure` once to save the server URL to `~/.config/qarax/config.toml`:

```bash
qarax configure
# Server URL [http://localhost:8000]: http://192.168.1.10:8000
# Saved to /home/user/.config/qarax/config.toml
```

Or pass `--server` non-interactively:

```bash
qarax configure --server http://192.168.1.10:8000
```

Server URL resolution order (highest to lowest priority):

1. `--server` flag or `QARAX_SERVER` environment variable
2. Value saved in `~/.config/qarax/config.toml`
3. Default: `http://localhost:8000`

## Global flags

| Flag | Description |
|------|-------------|
| `--server <URL>` | qarax API base URL (overrides config file) |
| `--json` | Print raw JSON instead of formatted tables |

## Resources accept names or IDs

All commands that reference an existing resource accept either the resource **name** or its **UUID**. The CLI tries to parse the argument as a UUID first; if that fails it looks up the resource by name.

```bash
# These are equivalent (assuming a VM named "my-vm" exists)
qarax vm start my-vm
qarax vm start 3f6c2b1a-0000-0000-0000-000000000001
```

---

## Virtual machines

```bash
# List all VMs
qarax vm list

# Get details of a VM
qarax vm get my-vm

# Create a VM (no image pull)
qarax vm create \
  --name my-vm \
  --vcpus 2 \
  --memory 1073741824 \
  --boot-source my-boot-source

# Create a VM from an OCI image (async, shows progress)
qarax vm create \
  --name my-vm \
  --vcpus 2 \
  --memory 2147483648 \
  --image-ref ghcr.io/example/my-vm-image:latest

# Start / stop / pause / resume
qarax vm start my-vm
qarax vm stop my-vm
qarax vm pause my-vm
qarax vm resume my-vm

# Print the VM console log
qarax vm console my-vm

# Attach an interactive console (WebSocket, exits with Ctrl-C)
qarax vm attach my-vm

# Delete a VM
qarax vm delete my-vm
```

---

## Hosts

```bash
# List all hosts
qarax host list

# Add a host
qarax host add \
  --name node-01 \
  --address 192.168.1.10 \
  --port 22 \
  --user root \
  --password secret

# Initialize a host (connects via gRPC, marks it UP)
qarax host init node-01

# Deploy a bootc image to a host
qarax host deploy node-01 \
  --image ghcr.io/example/qarax-node:latest \
  --ssh-key ~/.ssh/id_ed25519
```

---

## Storage pools

```bash
# List all storage pools
qarax storage-pool list

# Get details of a pool
qarax storage-pool get local-pool

# Create a local storage pool on a host
qarax storage-pool create \
  --name local-pool \
  --pool-type local \
  --host node-01 \
  --capacity 107374182400

# Delete a pool
qarax storage-pool delete local-pool
```

---

## Storage objects

```bash
# List all storage objects
qarax storage-object list

# Get details of an object
qarax storage-object get vmlinux

# Create a storage object (allocate space)
qarax storage-object create \
  --name my-disk \
  --pool local-pool \
  --object-type disk \
  --size 10737418240

# Delete an object
qarax storage-object delete my-disk
```

---

## Transfers

Transfers download remote files (or copy local files) into a storage pool.

```bash
# List transfers in a pool
qarax transfer list --pool local-pool

# Start a transfer
qarax transfer create \
  --pool local-pool \
  --name vmlinux \
  --source https://example.com/vmlinux \
  --object-type kernel

# Get transfer status
qarax transfer get --pool local-pool <transfer-uuid>
```

---

## Boot sources

A boot source combines a kernel object with optional kernel parameters and an initrd.

```bash
# List all boot sources
qarax boot-source list

# Get details of a boot source
qarax boot-source get my-boot-source

# Create a boot source (kernel and initrd referenced by name or ID)
qarax boot-source create \
  --name my-boot-source \
  --kernel vmlinux \
  --initrd test-initramfs \
  --params "console=ttyS0 reboot=k panic=1 pci=off"

# Delete a boot source
qarax boot-source delete my-boot-source
```

---

## Lifecycle hooks

```bash
# List hooks
qarax hook list

# Create a hook
qarax hook create \
  --name vm-events \
  --url https://example.com/webhook \
  --scope global

# Update a hook
qarax hook update vm-events \
  --url https://example.com/new-webhook \
  --secret new-secret

# Clear nullable hook fields
qarax hook update vm-events --clear-secret
qarax hook update vm-events --scope global --clear-scope-value
```

---

## Jobs

Long-running operations (e.g. OCI image pulls) run as async jobs. The `vm create` command polls the job automatically, but you can also inspect it directly.

```bash
qarax job get <job-uuid>
```

---

## End-to-end example

```bash
qarax configure --server http://localhost:8000

# 1. Add and initialize a hypervisor host
qarax host add --name node-01 --address 192.168.1.10 --user root --password secret
qarax host init node-01

# 2. Create a storage pool on the host
qarax storage-pool create --name local-pool --pool-type local --host node-01

# 3. Transfer a kernel and initrd into the pool
qarax transfer create --pool local-pool --name vmlinux \
  --source https://cloud-hypervisor.azureedge.net/vmlinux --object-type kernel
qarax transfer create --pool local-pool --name test-initramfs \
  --source https://example.com/test-initramfs.gz --object-type initrd

# 4. Create a boot source
qarax boot-source create --name my-boot-source --kernel vmlinux \
  --initrd test-initramfs --params "console=ttyS0 reboot=k panic=1 pci=off"

# 5. Create and start a VM
qarax vm create --name my-vm --vcpus 2 --memory 1073741824 \
  --boot-source my-boot-source
qarax vm start my-vm

# 6. Check status and attach console
qarax vm get my-vm
qarax vm attach my-vm
```
