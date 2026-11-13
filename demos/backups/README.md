# Backups Demo

Exercises the top-level `qarax backup` surface end-to-end:

1. `qarax backup create vm`, `list`, `get`, `restore`, and `delete` (which also
   removes the snapshot from the node)
2. `qarax backup create database`, `list`, `get`, and `restore`

Each half checks that the restore really brought state back:

- **VM:** the VM runs with `--guest-agent`. Before the backup, the demo writes
  `before-backup` to a file inside the guest with `qarax vm exec`; after it,
  it overwrites the file with `after-backup` and stops the VM. The guest's
  root filesystem is its initramfs, in RAM, so the file is gone once the VM
  stops. After `qarax backup restore` the file reads `before-backup` again:
  the guest's memory came back from the backup's snapshot.
- **Database:** one instance type is created before the database backup and
  another after it. After the restore the first still exists and the second
  doesn't. The backup's own record is gone too, since it was written after
  the dump.

## Prerequisites

- qarax stack running: `make run-local` (the script starts it via `./hack/run-local.sh` if it is not up)
- `jq`
- `docker` (the script creates the dump directory and clears dump files
  inside the `qarax` container of the local Compose stack)
- `qarax` CLI: built from this checkout when cargo is available, else taken from `PATH`

If your server uses a token other than the local default, export `QARAX_TOKEN`.

## Usage

```bash
./demos/backups/run.sh

# Keep the demo's resources if a step fails
KEEP=1 ./demos/backups/run.sh

# Delete everything the demo made, the reusable pool included
./demos/backups/run.sh --cleanup
```

Optional flags:

- `--server URL`: override the API endpoint
- `--pool-name NAME`: override the reusable local backup pool name (default `demo-backups-local-pool`)
- `--pool-path PATH`: override the writable control-plane-local dump path (default `/tmp/qarax-demo-backups`)
- `--cleanup`: delete the demo's backups, VM, instance types and pool, then exit

## Notes

- The demo is tuned for the repo's local Docker stack.
- Database backups require a **local** storage pool whose configured path is
  writable by the qarax control-plane process. By default this demo uses
  `/tmp/qarax-demo-backups` inside the running `qarax` container.
- The same pool holds the VM backup's snapshot, written by the node running
  the VM. The demo attaches the pool to every UP host
  (`storage-pool attach-host --all`), so it works wherever the VM lands.
- The pool is left in place for reuse across runs; `--cleanup` removes it.
- Resources are named with the `demo-backups-` prefix. Each run first deletes
  backups, VMs and instance types an earlier run left behind.
