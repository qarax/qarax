# BLOCK storage demo (iSCSI via LIO targetcli)

This demo brings up a LIO iSCSI target in a container and registers it with
qarax as a `BLOCK` storage pool.

**On the default local stack it exercises the API only** (pool creation and LUN
registration): the compose `qarax-node` can't log in to the target, so no LUN
reaches a node or a VM. See [Limitation](#limitation-no-iscsi-login-in-the-container-stack).

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
  `hack/run-local.sh` if the API is not reachable). `make run-local VM=1` is not
  supported: the target is only reachable on the compose network.
- `jq`, Docker with `docker compose`
- `qarax` CLI built in `target/` or on PATH, or a Rust toolchain so the demo can build it
- Host kernel with LIO (`target_core_mod`, `iscsi_target_mod`); the target uses
  the host's LIO via `/sys/kernel/config`

If your server uses a token other than the local default, export `QARAX_TOKEN`.

## Limitation: no iSCSI login in the container stack

The kernel's iSCSI initiator netlink interface only works in the host network
namespace. The compose `qarax-node` runs in a Docker bridge network, so its
login fails (`iscsid` logs `sendmsg: bug? ctrl_fd`) even though discovery
succeeds. The demo therefore shows the pool and LUN APIs and doesn't wait for a
session. On a real hypervisor host (bootc appliance or bare metal) the same pool
attaches and its LUNs appear under `/dev/disk/by-path/`.

## Run

```bash
./demos/block-storage/run.sh

# keep the pool, disk object and target running afterward
KEEP=1 ./demos/block-storage/run.sh

# remove what a KEEP=1 or interrupted run left behind
./demos/block-storage/run.sh --cleanup
```

The script:

1. builds a `targetcli`-based image and starts it as the `iscsi-target` service
   in the e2e compose project (`docker compose ... up --wait`, which returns once
   the target listens), exporting `iqn.2024-01.io.qarax:demo` on
   `iscsi-target:3260`
2. creates the `demo-block-pool` BLOCK pool pointing at the target
3. registers LUN 0 (1 GiB) as the disk object `demo-block-lun0` and checks it
   is a 1 GiB disk in that pool

## Cleanup

At exit the script deletes the disk object, detaches the pool from every `up`
host (logging the nodes out of the target, where they logged in), deletes the
pool, and removes the target container. The target removes its LIO objects
from the host kernel when it stops. At start it deletes any `demo-block-*`
pool and disk object an earlier run left, and reuses a target container that is
still running.

With `KEEP=1` nothing is removed at exit; `--cleanup` removes it all later.

The 1 GiB sparse backing file lives in the `block_demo_storage` compose volume
and is reused on the next run. Remove it with
`docker volume rm e2e_block_demo_storage` once the target container is gone.
