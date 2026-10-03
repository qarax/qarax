# BLOCK storage demo (iSCSI via LIO targetcli)

This demo brings up a LIO iSCSI target in a container and registers it with
qarax as a `BLOCK` storage pool.

## What it shows

- A `BLOCK` pool is a network-attached iSCSI target. Shared pools auto-attach to
  every `up` host, so qarax-node runs iSCSI discovery + login against the target
  and each LUN appears on the node as
  `/dev/disk/by-path/ip-<portal>-iscsi-<iqn>-lun-<N>`.
- Because iSCSI is reachable from any initiator, a `BLOCK` pool is treated as
  shared storage (`supports_live_migration == true`).
- LUNs are pre-provisioned on the target. qarax does not create disks in a
  `BLOCK` pool; you register existing LUNs with `storage-pool register-lun`.

## Requirements

- qarax stack in container mode (`make run-local`; the script starts it via
  `hack/run-local.sh` if the API is not reachable). `--vm` mode is not supported.
- `jq`, Docker with `docker compose`
- `qarax` CLI built in `target/` or on PATH, or a Rust toolchain so the demo can build it
- Host kernel with LIO (`target_core_mod`, `iscsi_target_mod`) and `iscsi_tcp`;
  the target uses the host's LIO via `/sys/kernel/config`

If your server uses a token other than the local default, export `QARAX_TOKEN`.

## Limitation: no iSCSI login in the container stack

The kernel's iSCSI initiator netlink interface only works in the host network
namespace. The compose `qarax-node` runs in a Docker bridge network, so its
login fails (`iscsid` logs `sendmsg: bug? ctrl_fd`) even though discovery
succeeds. The demo therefore shows the pool and LUN APIs and reports that no
session was established. On a real hypervisor host (bootc appliance or bare
metal) the same pool attaches and its LUNs appear under `/dev/disk/by-path/`.

## Run

```bash
./demos/block-storage/run.sh

# keep the pool, disk object and target running afterward
./demos/block-storage/run.sh --keep-resources
```

The script:

1. builds a `targetcli`-based image and starts it as the `iscsi-target` service
   in the e2e compose project, exporting `iqn.2024-01.io.qarax:demo` on
   `iscsi-target:3260`
2. checks that `iscsid` is running in the `qarax-node` container (the node
   entrypoint starts it)
3. creates a BLOCK pool pointing at the target and reports whether `qarax-node`
   got an iSCSI session (it can't in the container stack; see above)
4. registers LUN 0 (1 GiB) as a disk object

## Cleanup

By default the script deletes the disk object, detaches the pool from every
`up` host (logging the nodes out of the target), deletes the pool, and removes
the target container. The target removes its LIO objects from the host kernel
when it stops.

With `--keep-resources`, tear down manually in the same order: delete the disk
object, `qarax storage-pool detach-host <pool> <host>` for each host, delete the
pool, then stop the target:

```bash
docker compose -f e2e/docker-compose.yml -f demos/block-storage/compose.yml rm -sf iscsi-target
```

The 1 GiB sparse backing file lives in the `block_demo_storage` compose volume
and is reused on the next run. Remove it with
`docker volume rm e2e_block_demo_storage` once the target container is gone.
