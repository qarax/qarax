# Forking sandboxes

`POST /v1/sandboxes/{id}/fork` (`qarax sandbox fork SANDBOX --count N`) makes
new sandboxes that continue from a running sandbox's current state: its
memory, its running processes and its files, whether in a RAM-backed root or
on the disk of the OCI image it booted. They run
on the same host and are ready when the call returns; the source keeps
running. An agent can branch mid-task to try several approaches from the same
point, or fan out from a warmed sandbox (imports done, caches loaded) without
booting each copy.

```bash
qarax sandbox fork my-sandbox --count 3
```

## What forks

- Firecracker sandboxes with the guest agent, no network interfaces, no
  cloud-init and no disk but the OCI image they booted, if they booted one. A
  NIC would give the forks the source's MAC and IP addresses; any other disk
  (a cloud-init seed, a persistent OverlayBD layer) would have them write to
  the same files. The image disk forks with the sandbox, see below.
- Up to 16 forks per request. The source's host must have room for them: the
  scheduler counts each fork at the source's full memory size and vCPUs, as it
  does any VM, though a fork uses much less at first (see below).
- A fork gets the source's template, idle timeout and VM configuration, and
  the name `<source>-fork-<8 hex digits>`.

## How it works

1. The node pauses the source, writes a full Firecracker snapshot of it
   (memory and device state) and resumes it. The pause lasts as long as
   writing the memory does, about 0.1 s for 256 MiB on fast storage.
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
   file mapped.

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

Either every fork of a request starts or none does. The forks' rows point at
the source's image storage object, so the database shows what they boot.

## What it costs

Measured on a laptop with a 256 MiB guest, the node in a container capped at
2 CPUs ("bare") and in the bootc node microVM under Cloud Hypervisor with
2 vCPUs ("nested", as on a KubeVirt node):

| | bare | nested |
|---|---:|---:|
| Cold boot of the source, until the guest agent answers | 0.37 s | 1.5 s |
| Source paused for the snapshot | 116 ms | 130 ms |
| One fork, until its guest agent answers | 13–18 ms | 66–72 ms |
| A fork's own memory after it starts | ~7 MiB | ~7 MiB |
| Extra pause for an OCI disk (flush, write-back, reflink) | | 8–31 ms |

The disk figure is from the prototype, on XFS; on a filesystem without
reflinks the copy takes as long as copying the source's writable layer. Each
fork's disk device costs overlaybd-tcmu about 7 MiB of memory.

A fork's memory grows by what it writes. Data it reads from a disk or
re-reads after its page cache drops also becomes its own; the page cache it
inherits stays shared.

## Not yet

- **NICs.** Forks keep their source's MAC and IP addresses, so each would need
  a network namespace of its own with those addresses inside and its own
  address outside.
- **Cloud Hypervisor** sandboxes.
- **Diff snapshots**, so that forking a source again writes only the memory it
  changed since the last time.
