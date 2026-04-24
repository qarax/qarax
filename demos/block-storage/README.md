# BLOCK storage demo (iSCSI via LIO targetcli)

This demo brings up a LIO iSCSI target in a container and registers it with
qarax as a `BLOCK` storage pool.

## What it shows

- A `BLOCK` pool is a network-attached iSCSI target. qarax-node logs in to the
  target and each LUN appears as `/dev/disk/by-path/ip-<portal>-iscsi-<iqn>-lun-<N>`.
- Because iSCSI is reachable from any initiator, a `BLOCK` pool is treated as
  shared storage (`supports_live_migration == true`).
- LUNs are pre-provisioned on the target. qarax does not create disks in a
  `BLOCK` pool; you register existing LUNs with `storage pool register-lun`.

## Requirements

- qarax stack running (`make run-local`)
- qarax CLI on PATH
- Docker (the target uses kernel LIO via `/sys/kernel/config` on the host)

## Run

```
./demos/block-storage/run.sh
```

The script builds a `targetcli`-based container image, brings it up in the e2e
compose network, creates a BLOCK pool pointing at `iqn.2024-01.qarax:demo` on
`iscsi-target:3260`, and registers LUN 0 (1 GiB) as a disk object.

## Teardown

```
(cd e2e && docker compose \
    -f docker-compose.yml \
    -f ../demos/block-storage/compose.yml \
    down iscsi-target)
```
