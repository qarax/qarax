# OCI Images in Qarax

Qarax can boot VMs directly from OCI container images using OverlayBD, a lazy-loading
block device format. This document explains how it works end-to-end.

## Two OCI Boot Paths

Qarax has two mechanisms for using OCI images:

| Path | Transport | Block device? | Use case |
|------|-----------|---------------|----------|
| **OverlayBD** | TCMU block device | Yes (`/dev/vda`) | Full VMs with persistent or ephemeral disks |
| **ImageStore** | virtiofsd (virtio-fs) | No (filesystem mount) | Container-like workloads |

This document covers the OverlayBD path. It is the primary path for disk-based VMs.

---

## Architecture Overview

The key idea is **copy-on-write layering**:

```
OCI Registry (shared, read-only)
       │
       │  lazy block I/O (on-demand fetch)
       ▼
overlaybd-tcmu (kernel TCMU daemon)
       │
       ├── Lower layers: OCI image blobs from registry   ← shared across VMs
       └── Upper layers: writable .data + .index files   ← one set per VM
                  │
                  ▼
           /dev/vda  (presented to Cloud Hypervisor as a normal block device)
```

Multiple VMs can run from the same OCI image simultaneously. They share the read-only
lower layers from the registry, while each VM writes to its own independent upper layer.
The registry acts as the shared, immutable foundation; upper layers capture per-VM state.

---

## Lifecycle

### 1. Import

Before a VM can use an OCI image, it must be imported into an OverlayBD storage pool:

```bash
qarax storage-pool import --pool overlaybd-pool --image-ref alpine:latest --name alpine-obd
```

This triggers an async background job that:

1. Copies the source image to the pool's local OCI registry (`registry_url` from pool config)
   via `oci-client`.
2. Converts it to OverlayBD format using the `convertor` binary
   (from [accelerated-container-image](https://github.com/containerd/accelerated-container-image)).
   This rewrites the layer manifests into the lazy-loadable OverlayBD format.
3. Stores a `StorageObject` of type `OciImage` in the database:
   ```json
   {
     "image_ref": "registry:5000/docker/library/alpine:latest",
     "registry_url": "http://registry:5000",
     "digest": "sha256:..."
   }
   ```

No block device is created yet — the import just prepares the registry.

### 2. Create VM

When you create a VM and attach an OCI image as a disk, another async job runs:

1. Creates a `VmDisk` record linking the `OciImage` storage object to the VM.
2. If `persistent_upper_pool_id` is specified, creates a `StorageObject` of type
   `OverlaybdUpper` in that pool. This object records the paths to the writable upper
   layer files for this VM:
   ```json
   {
     "upper_data": "/pools/nfs-pool/uuid.upper.data",
     "upper_index": "/pools/nfs-pool/uuid.upper.index"
   }
   ```
   The `VmDisk` record links to this upper object via `upper_storage_object_id`.
3. If no `persistent_upper_pool_id` is given, `upper_storage_object_id` is `NULL` —
   the VM is ephemeral (all writes lost on stop).

### 3. Start

When the VM starts, the control plane builds a `CreateVmRequest` proto with `DiskConfig`:

```protobuf
DiskConfig {
  id: "vda"
  oci_image_ref: "registry:5000/docker/library/alpine:latest"
  registry_url: "http://registry:5000"
  upper_data_path: "/pools/nfs-pool/uuid.upper.data"   // null if ephemeral
  upper_index_path: "/pools/nfs-pool/uuid.upper.index"  // null if ephemeral
}
```

The node's `OverlayBdManager::mount()` then:

1. **Creates the writable upper layer** by running `overlaybd-create`:
   - Produces sparse `upper.data` and `upper.index` files (64 GB sparse by default)
   - Stored on NFS/local pool if persistent, else in `/var/lib/qarax/overlaybd/{vm_id}/`

2. **Fetches layer descriptors** from the OverlayBD-converted manifest in the registry.
   Each layer is described by its digest, size, and fetch URL.

3. **Writes a TCMU config** at `/var/lib/qarax/overlaybd/{vm_id}/config.json`:
   ```json
   {
     "repoBlobUrl": "http://registry:5000/v2/docker/library/alpine/blobs/",
     "lowers": [
       { "digest": "sha256:...", "size": 12345, "dir": "" }
     ],
     "upper": {
       "index": "/pools/nfs-pool/uuid.upper.index",
       "data": "/pools/nfs-pool/uuid.upper.data"
     }
   }
   ```

4. **Creates a TCMU backstore** in configfs under
   `/sys/kernel/config/target/core/user_1/obd-{id}`. Writing `enable=1` signals the
   `overlaybd-tcmu` daemon, which reads the config and opens the backing files.

5. **Sets up a loopback SCSI fabric** via `tcm_loop`. A LUN symlink connects the fabric
   to the TCMU backstore. The kernel creates a `/dev/sd*` block device.

6. Returns the device path (`/dev/sdb`, etc.) to Cloud Hypervisor, which presents it to
   the VM as `/dev/vda`.

The kernel command line is set to:
```
console=ttyS0 root=/dev/vda rw net.ifnames=0 biosdevname=0 init=/.qarax-init
```

`init=/.qarax-init` overrides the image's `/sbin/init`. `qarax-init` is injected into
the rootfs and executes the OCI entrypoint/cmd from the image config.

### 4. Running

While the VM is running:

- **Reads** that hit uncached blocks are fetched lazily from the registry by the
  `overlaybd-tcmu` daemon (block-level, not file-level).
- **Writes** go to `upper.data` / `upper.index` only. The registry content is never
  modified.
- Blocks fetched from the registry are **not cached locally** between restarts — they
  are re-fetched as needed from the registry on the next start.

### 5. Stop

When the VM stops, the node tears down:

1. The loopback fabric (LUN symlink, TPG, WWN configfs directories)
2. The TCMU backstore (`enable=0`, remove configfs directory)
3. The per-VM config directory (`/var/lib/qarax/overlaybd/{vm_id}/`)

If the upper layer is persistent, `upper.data` and `upper.index` on the NFS/local pool
are **left intact**. On next start, the same upper layer is reused, so the VM continues
from where it left off.

If the upper layer is ephemeral, it is deleted and all VM writes are discarded.

---

## Sharing the Same Image Across VMs

Multiple VMs can boot from the same OCI image at the same time:

```
Import once:
  alpine:latest → registry:5000/docker/library/alpine:latest (OverlayBD format)

VM1:
  DiskConfig { oci_image_ref: "registry:.../alpine:latest", upper_data: "/nfs/vm1.upper.data" }
  /dev/vda = lower layers (registry) + vm1.upper.data  ← VM1's writes

VM2:
  DiskConfig { oci_image_ref: "registry:.../alpine:latest", upper_data: "/nfs/vm2.upper.data" }
  /dev/vda = lower layers (registry) + vm2.upper.data  ← VM2's writes, completely isolated
```

The lower layers are identical — the registry serves the same blobs to both VMs.
The upper layers are independent — writes in VM1 are invisible to VM2 and vice versa.

The only shared resource is the registry, which is read-only from the VMs' perspective.

---

## Persistent vs. Ephemeral VMs

Controlled by `upper_storage_object_id` in the `vm_disks` table:

| `upper_storage_object_id` | Upper layer location | Survives stop? |
|--------------------------|----------------------|----------------|
| `NULL` | `/var/lib/qarax/overlaybd/{vm_id}/` (host-local temp) | No |
| `uuid` → `OverlaybdUpper` SO | NFS or local pool path from SO config | Yes |

Ephemeral is efficient for stateless workloads (e.g., batch jobs, CI runners). Persistent
is needed for VMs that accumulate state (e.g., databases, long-running services).

---

## Storage Pool Configuration

OverlayBD pools require an OCI registry:

```json
{
  "url": "http://registry:5000"
}
```

The pool is marked `shared = true` and `supports_live_migration = true` in the database,
meaning the same pool can be attached to multiple hosts and VMs can be live-migrated
between hosts (the registry is reachable from all hosts; the upper layer must be on a
shared pool like NFS for live migration to work).

---

## Limitations

- **No resize**: OverlayBD disks cannot be resized after creation.
- **No export**: There is no built-in mechanism to convert an OverlayBD VM's disk to a
  raw or qcow2 image. See the next section for discussion.
- **Registry dependency**: VMs depend on the registry being reachable at boot time for
  blocks that are not yet in the upper layer. If the registry is unavailable, cold reads
  will stall.
- **Upper layer grows unboundedly**: Writes accumulate in `upper.data` indefinitely.
  There is no compaction or garbage collection.
