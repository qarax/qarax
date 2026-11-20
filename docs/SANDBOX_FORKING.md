# Forking sandboxes

`POST /v1/sandboxes/{id}/fork` (`qarax sandbox fork SANDBOX --count N`) makes
new sandboxes that continue from a running sandbox's current state: its
memory, its running processes and its files, whether in a RAM-backed root or
on the disk of the OCI image it booted. They run on the same host, each with
addresses of its own on the source's networks, and are ready when the call
returns; the source keeps running. An agent can branch mid-task to try several approaches from the same
point, or fan out from a warmed sandbox (imports done, caches loaded) without
booting each copy.

```bash
qarax sandbox fork my-sandbox --count 3
```

## What forks

- Firecracker and Cloud Hypervisor sandboxes with the guest agent, no network interface but ones
  on a managed bridge or isolated network, and no writable disk but the OCI
  image they booted, their cloud-init seed and disks on local or NFS pools.
  Any other NIC would leave the forks with the source's addresses; any other
  writable disk (a persistent OverlayBD layer, an iSCSI LUN) would have them
  write to the same place. The image disk, the data disks, the NICs and the
  seed fork with the sandbox, and read-only disks are shared, see below.
- Up to 16 forks per request. The source's host must have room for them: the
  scheduler counts each fork at the source's full memory size and vCPUs, as it
  does any VM, though a fork uses much less at first (see below).
- A fork gets the source's template, idle timeout and VM configuration, and
  the name `<source>-fork-<8 hex digits>`.

## How it works

1. The node pauses the source, writes a Firecracker snapshot of it (memory
   and device state) and resumes it. The first time, it writes all of the
   memory: the pause lasts as long as that does, about 0.1 s for 256 MiB on
   fast storage. The snapshot isn't synced to disk: the forks read it from the
   page cache on the same host.
2. Each fork is a new Firecracker process that loads the snapshot with the
   `File` memory backend, which maps the memory file `MAP_PRIVATE`: the forks
   share the source's pages until they write them. Each fork's vsock gets its
   own socket (`vsock_override`); the guest keeps the source's CID, which only
   it sees.
3. A fork resumes with its source's random generator state and clock. The
   node sends each fork's guest agent a fork notice with 64 bytes from the
   node's `/dev/urandom` and the time: qarax-init credits the bytes to the
   kernel's pool, reseeds the kernel's generator from it (`RNDADDENTROPY`,
   `RNDRESEEDCRNG`) and sets the clock. A qarax-init older than forking refuses
   the notice, and the fork fails. Random generators in guest processes
   (OpenSSL, numpy, ...) keep the state they had in the source: reseed them if
   the forks need their own.
4. The snapshot is deleted once the forks have loaded it: they keep the memory
   file mapped. A source with the guest agent keeps its memory image as its
   fork base, below.

A Cloud Hypervisor fork restores the snapshot copy-on-write
(`memory_restore_mode=copyonwrite`): its memory maps the snapshot's memory file
privately, as a Firecracker fork's does, so the forks share the source's pages
until they write them. The file has to stay as it is for as long as a fork
runs, so each fork restores from a directory of its own in the node's runtime
directory, with hard links to the snapshot's memory and device state and the
snapshot's config rewritten for the fork (its vsock socket, TAPs and disk
files); the directory goes with the fork. Copy-on-write needs plain private
guest memory: a VM with shared memory or huge pages (for vhost-user devices,
say) restores with a copy of its memory instead.

A Firecracker sandbox forked again writes only the memory it dirtied since its
last fork; a Cloud Hypervisor sandbox writes all of it each time.
Firecracker tracks the pages a VM with the guest agent dirties, from boot (or,
for a fork, from its load). The node keeps the memory image of a source's last
fork round, its fork base, in the node's runtime directory; the next round
takes a diff snapshot, the pages dirtied since, so the pause lasts as long as
writing those does. Once the source runs again, the node lays them over a copy
of the base (a reflink where the filesystem has them, a copy of the memory
otherwise) and the forks load that, the new base. A base is never written to:
the forks that loaded it map it, and keep it when it's replaced. One fork round
of a source runs at a time, and a snapshot of the source through the API or a
restart forgets the base, so its next fork takes a full snapshot again; so
does the first fork of a VM a restarted node recovered. A source forked once
keeps a file the size of its memory in the runtime directory until it stops.

A source booted from an OCI image forks its disk too. The image is OverlayBD
read-only layers with a writable layer of the VM's own on top:

1. While the source runs, the node writes back the writable layer's dirty
   pages: overlaybd-tcmu leaves its writes in the page cache.
2. During the pause, after the memory snapshot: the node flushes the source's
   disk device, writes back what is left, and copies the writable layer, a
   reflink where the filesystem has them (XFS, btrfs), so the copy takes
   milliseconds whatever its size.
3. Once the source runs again, the copy is sealed into a read-only layer
   (`overlaybd-commit --seal`), and each fork gets a disk of its own: the
   image's layers, the sealed one, and an empty writable layer. Each fork's
   OverlayBD directory holds a hard link to the sealed layer, so it lasts as
   long as one of those forks does.
4. Each fork loads the snapshot paused, its drive is pointed at its own disk,
   and then it resumes. A write that was still in the source's page cache at
   the fork is in each fork's memory and reaches the fork's own disk.

A source's network interfaces fork too, each onto the same network with the
fork's own addresses:

1. The control plane gives each fork a copy of each of the source's NICs,
   with a new address from the network's pool, and the source's security
   groups. The node gives each fork's NIC its own MAC, as it does any VM's.
2. Each fork loads the snapshot paused onto TAPs of its own
   (`network_overrides`), attached to the source NICs' bridges. The node writes
   the forks' firewall (anti-spoofing for their own MAC and address, and their
   security groups), then resumes them.
3. A fork's guest still has the source's MAC and address, which the fork's
   firewall drops. After the fork notice, the node sends the guest agent the
   fork's addresses: for each NIC, the guest interface that has the source's
   MAC is taken down, gets the fork's MAC, address and netmask, comes up, and
   gets its default route back. A qarax-init older than this refuses, and the
   fork fails.

The guest's connections from before the fork carry the source's address in
the fork, so a fork's TCP connections to other machines break: it reconnects
from its own address. Only guests whose addresses come from the kernel
command line (`ip=`), as qarax sets them up, are re-addressed; a DHCP client or
network manager in the guest keeps its own idea of the source's lease, and a
sandbox whose network config comes from cloud-init doesn't fork with its NICs.

A source with cloud-init forks its seed image: each fork gets its own, with the
same content, so it boots again after its source and its source's seed are
gone. The guest doesn't run cloud-init again: it already ran in the source.

A source's writable disks on local and NFS pools are copied: each fork gets a
disk storage object of its own in the same pool, which goes with it when it's
deleted, its file with it. The node writes back the source's dirty pages
while it runs, copies each disk during the pause into the first fork's file,
and from that file into the other forks' once the source runs again. A copy is
a reflink where the filesystem has them (XFS, btrfs), and takes milliseconds;
elsewhere it copies the disk's data, its holes left out, and the pause lasts as
long as copying one disk's data does. Read-only disks, an ISO say, are shared.

Either every fork of a request starts or none does. Each fork has its own copy
of the source's image storage object, which goes with it when it's deleted.

## What it costs

Measured with Firecracker on a laptop with a 256 MiB guest, the node in a container capped at
2 CPUs ("bare") and in the bootc node microVM under Cloud Hypervisor with
2 vCPUs ("nested", as on a KubeVirt node):

| | bare | nested |
|---|---:|---:|
| Cold boot of the source, until the guest agent answers | 0.37 s | 1.5 s |
| Source paused for the snapshot (the first fork) | 116 ms | 130 ms |
| One fork, until its guest agent answers | 13–18 ms | 66–72 ms |
| A fork's own memory after it starts | ~7 MiB | ~7 MiB |
| Extra pause for an OCI disk (flush, write-back, reflink) | | 8–31 ms |
| Extra pause for a data disk without reflinks | | the time to copy its data |

The disk figure is from the prototype, on XFS; on a filesystem without
reflinks the copy takes as long as copying the source's writable layer. Each
fork's disk device costs overlaybd-tcmu about 7 MiB of memory.

A fork's memory grows by what it writes. Data it reads from a disk or
re-reads after its page cache drops also becomes its own; the page cache it
inherits stays shared.
