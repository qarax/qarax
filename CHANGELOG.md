# Changelog

Notable changes to qarax, by release. Versions follow [Semantic
Versioning](https://semver.org): before 1.0, a minor release (0.x) may change
the REST API, the control plane to node protocol and the database schema in
ways that need action on upgrade, and its notes say what. See "Upgrading" in
[docs/INSTALL.md](docs/INSTALL.md).

## [0.1.0] - Unreleased

The first release.

### What's in it

- VMs on Cloud Hypervisor and Firecracker: create, start, stop, pause, resume,
  resize (CPU, memory, disks), hot-plug disks and NICs, console and `exec`
  through the guest agent, cloud-init.
- Images: OCI images booted lazily through OverlayBD (on Cloud Hypervisor and
  Firecracker), cloud and disk images, kernel boot sources, instance types and
  VM templates.
- Storage pools: local, NFS, OverlayBD and iSCSI block, with capacity
  accounting and durable cleanup of the files qarax provisions. A storage
  object a VM makes for itself, such as the image object of a VM booted from
  an OCI image, is deleted with the VM unless something still uses it.
- Networking: bridged and isolated networks with IPAM, DHCP and DNS,
  VPC-style isolation with VXLAN overlays across hosts, per-NIC security
  groups, and disk and NIC rate limits.
- Hosts: deployment and upgrades as bootc appliances over SSH, maintenance and
  evacuation, placement policies (labels, affinity, spread), GPU passthrough
  and NUMA placement.
- Live migration (TLS, postcopy), HA failover of VMs from dead hosts.
- Snapshots and backups of VMs, and backups of the control-plane database.
- Sandboxes: short-lived VMs with idle reaping and prewarmed pools, which can
  make their sandboxes by forking one booted VM of the template, and forks of
  a running Firecracker or Cloud Hypervisor sandbox, its OCI image disk, data
  disks, network interfaces and cloud-init included, that continue from its
  state; forking a Firecracker sandbox again writes only the memory it changed
  since, and a fork counts against its host's memory for what it holds of its
  own, not its memory size ([docs/SANDBOX_FORKING.md](docs/SANDBOX_FORKING.md)).
- Lifecycle hooks (webhooks), a Server-Sent Events stream of VM changes, an
  audit log of API mutations, OpenTelemetry traces and metrics.
- A CLI (`qarax`) and a generated Python SDK.
- A web UI at `/ui`: the fleet at a glance; VMs, hosts (each with its VMs,
  capacity, placement, NUMA nodes and GPUs, and maintenance and evacuation),
  networks (their hosts and the addresses VMs hold) and storage pools (their
  hosts, objects and transfers); creating a VM from an OCI image, a kernel or
  a disk, sized by vCPUs and memory or an instance type, on a network with
  security groups; a VM's details and actions (start, stop, pause, resume,
  force stop, delete), live charts of its disk and network throughput, and its
  serial console in the browser, all kept current from the event stream. For
  clients that can't set the Authorization header on a WebSocket, as browsers
  can't, the console also takes the API token as the subprotocol
  `qarax.bearer.<the token, hex-encoded>`.

### Changed since pre-release builds of master

These need action if you ran a build from before them:

- The REST API is under `/v1` (`/v1/vms`, ...). Only `/`, `/swagger-ui` and
  `/api-docs/openapi.json` stay at the root.
- Lists that grow with use return `{"items": [...], "next_cursor": ...}` and
  take `limit` (1-1000, default 100) and `cursor`, instead of a bare array.
  The audit log's `limit` now refuses values above 1000.
- qarax-node serves gRPC only over mutual TLS: each node needs its
  certificates in `--tls-dir` and the control plane its client certificate in
  `node_tls_dir` (`scripts/gen-node-tls.sh`, see the README). Neither
  starts without them.
- The database migrations are squashed into one: a database created by an
  earlier build won't start, so recreate it.
- Deleting a VM deletes its snapshots and VM backups, with their data.
  `DELETE /v1/vms/{id}/snapshots/{snapshot_id}` deletes a single snapshot.
- Restoring a database backup keeps the database backups' records as they
  were before the restore.
- An OverlayBD import writes the converted image to `<tag>_overlaybd` and
  leaves the mirrored image as it was; earlier builds converted the mirror in
  place, so importing it again gave an empty disk. An image you pushed to the
  local registry that an earlier build imported holds OverlayBD layers under
  its own tag now: push it again before importing it.
- The control plane's database pool follows `database.max_connections` (50
  in `production.yaml`) and `database_name`. It was 10 connections whatever
  the setting said, on the database named after the user. Postgres's own
  `max_connections` (100 unless changed) has to allow the pool's.
- A BLOCK pool's portal is an IP address, optionally with a port (3260 when
  left out); a hostname is refused. A VM opens a LUN by the device name udev
  gives it, which holds the address and port the node logged in to. A LUN of
  a pool created with a hostname portal can't be used: create the pool again
  with the target's address and register its LUNs again.
