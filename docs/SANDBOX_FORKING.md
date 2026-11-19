# Forking sandboxes

`POST /v1/sandboxes/{id}/fork` (`qarax sandbox fork SANDBOX --count N`) makes
new sandboxes that continue from a running sandbox's current state: its
memory, its running processes and the files in its RAM-backed root. They run
on the same host and are ready when the call returns; the source keeps
running. An agent can branch mid-task to try several approaches from the same
point, or fan out from a warmed sandbox (imports done, caches loaded) without
booting each copy.

```bash
qarax sandbox fork my-sandbox --count 3
```

## What forks

- Firecracker sandboxes with the guest agent and no network interfaces or
  disks (a cloud-init seed is a disk). A fork shares everything but memory
  with its source: a NIC would give the forks its MAC and IP addresses, a disk
  would have them write to the same files.
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

Either every fork of a request starts or none does.

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

A fork's memory grows by what it writes. Data it reads from a disk or
re-reads after its page cache drops also becomes its own; the page cache it
inherits stays shared.

## Not yet

- **NICs.** Forks keep their source's MAC and IP addresses, so each would need
  a network namespace of its own with those addresses inside and its own
  address outside.
- **Disks.** A Firecracker sandbox can boot an OCI image, but forking it needs
  its disk forked too: the source's upper layer frozen in place (write back its
  dirty pages, reflink-clone it during the pause and `overlaybd-commit --seal`
  the clone) with a fresh upper for each fork on top. A prototype measured
  8–31 ms of extra pause for that.
- **Cloud Hypervisor** sandboxes.
- **Diff snapshots**, so that forking a source again writes only the memory it
  changed since the last time.
