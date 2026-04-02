VM Lifecycle

  - Security groups / firewall rules — no ACLs or per-VM network policies

  - VPC / subnet isolation — only flat bridge-based networking
  - Independent volume snapshots — only full VM snapshots, not per-disk
  - Volume cloning — no way to clone a disk independently
  - Snapshot scheduling / policies — no automated snapshot management
  - Disk attach to stopped VMs — hotplug only works on running VMs

  - Resource quotas — no per-tenant or per-user limits
  - Affinity / anti-affinity rules — no VM placement constraints beyond GPU filtering

  - VM export / backup — no way to export a VM image
  - Nested virtualization — not exposed as a configurable option
  - USB passthrough — only VFIO/GPU passthrough exists
  - Health checks / probes — no guest-level liveness monitoring



  - POST /sandboxes/{id}/exec — command execution inside
    the VM
  - Pre-warming / VM pools
  - Snapshot-based fast restore
  - Concurrency limits / autoscaling
  - E2E tests (added after the exec endpoint exists)


OCI / Persistent Storage

  - Full clone (OCI → raw disk): convert an OCI image to a flat raw disk file
    at VM creation time. Stores as StorageObject(type=Disk) on Local/NFS pool.
    Fully portable, resizable, migratable — no registry dependency after creation.
    Default size: max(sum_of_uncompressed_layers * 2, 4 GiB), user-overridable.

  - Resize persistent OverlayBD upper layer: overlaybd-create sets a fixed
    virtual size. To support resize, expose the size parameter at creation and
    implement re-creation of the upper layer with a larger size (offline).

  - Disk resize for OverlayBD persistent VMs: extend resize_disk to support
    resizing the OverlayBD upper layer (currently Local/NFS raw files only).

  - Snapshot of persistent OverlayBD VMs: copy upper.data + upper.index into
    a new OverlaybdUpper StorageObject (parent = current upper SO).

  - Ephemeral upper layer size: make configurable at VM creation time
    (currently hardcoded to 64 GiB virtual in overlaybd-create).


Platform

  - Web UI — full management interface with console, graphs, VM lifecycle
  - RBAC / Auth — users, groups, roles, LDAP, API tokens, 2FA
  - High Availability — automatic failover, fencing, cluster config replication
  - Audit log — record all API mutations with actor and before/after state
  - Resource pools — group VMs/hosts for delegation, billing, quotas
  - Scheduled backups — cron-based snapshot scheduling with retention policies
  - Memory ballooning — virtio-balloon API for memory reclaim
  - TPM support — vTPM via swtpm

- make run-local: suggests IDs instead of names - make project wide pass
- improve VM create with image import
- make sure all demos are standalone and idempotent
