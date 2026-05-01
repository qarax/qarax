# Backups Demo

This demo exercises the new top-level `qarax backup` surface end-to-end.

It covers:

1. `qarax backup create vm`, `list`, `get`, and `restore`
2. `qarax backup create database`, `list`, `get`, and `restore`

The database half proves that restoring a control-plane backup really rewinds
state by keeping one instance type created before the backup and removing a
second one created after it.

## Prerequisites

- `./hack/run-local.sh`
- `jq`
- Docker with `docker compose`

## Usage

```bash
./demos/backups/run.sh
```

Optional flags:

- `--server URL` — override the API endpoint
- `--host NAME` — choose which UP host the reusable local backup pool attaches to
- `--pool-name NAME` — override the reusable local backup pool name
- `--pool-path PATH` — override the writable control-plane-local dump path

## Notes

- The demo is tuned for the repo's local/e2e Docker stack.
- Database backups require a **local** storage pool whose configured path is
  writable by the qarax control-plane process. By default this demo uses
  `/tmp/qarax-demo-backups` inside the running `qarax` container.
- The demo leaves the local backup pool in place for reuse across runs.
- After a database restore, the backup metadata created after the restore point
  disappears by design; the script verifies that behavior.
