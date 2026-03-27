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
