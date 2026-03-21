VM Lifecycle

  - Reboot — no dedicated operation; requires stop + start
  - Force stop/kill — only graceful shutdown, no forced power-off with timeout
  - Reset — no hard reset equivalent

  Networking

  - Security groups / firewall rules — no ACLs or per-VM network policies
  - VPC / subnet isolation — only flat bridge-based networking
  - Floating IP attach/detach API — IPs are managed per-network but can't be reassigned via API

  Storage

  - Independent volume snapshots — only full VM snapshots, not per-disk
  - Volume cloning — no way to clone a disk independently
  - Snapshot scheduling / policies — no automated snapshot management
  - Disk attach to stopped VMs — hotplug only works on running VMs

  Organization & Multi-tenancy

  - VM tags / labels — no metadata tagging system
  - Resource quotas — no per-tenant or per-user limits
  - Affinity / anti-affinity rules — no VM placement constraints beyond GPU filtering

  Operations

  - VM export / backup — no way to export a VM image
  - Nested virtualization — not exposed as a configurable option
  - USB passthrough — only VFIO/GPU passthrough exists
  - Health checks / probes — no guest-level liveness monitoring


qarax-node updates - inform out of date nodes, update mechanism, etc
