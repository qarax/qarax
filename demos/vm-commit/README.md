# VM Commit Demo

Demonstrates the `vm commit` workflow: converting an OCI image-backed VM
(OverlayBD) into a standalone raw-disk VM.

## What it does

1. Ensures the qarax stack is running (starts it via `./hack/run-local.sh` if not)
2. Creates a Local storage pool `demo-commit-local` on the first UP host for the
   committed raw disk (reused on later runs)
3. Creates a VM with `--image-ref public.ecr.aws/docker/library/busybox:latest`;
   qarax imports the image into the shared `overlaybd-pool`
4. Runs `vm commit` to byte-copy the OverlayBD block device to a 1 GiB raw disk
5. Verifies that `image_ref` is cleared and that the committed disk object exists
6. Starts the committed VM and checks it is running

Each check stops the demo with an error if it fails.

## Prerequisites

- qarax stack running: `make run-local` (the script starts it if it is not running).
  It registers the host and creates `overlaybd-pool`, which this demo uses and
  never deletes.
- `cargo` (the script builds the `qarax` CLI) or a `qarax` CLI on PATH
- `jq`

If your server uses a token other than the local default, export `QARAX_TOKEN`.

## Usage

```bash
# Start the stack if not running, then run the demo
./demos/vm-commit/run.sh

# Custom server URL
QARAX_SERVER=http://localhost:8000 ./demos/vm-commit/run.sh

# Delete the VM, the committed disk and the local pool
./demos/vm-commit/run.sh --cleanup
```

The demo leaves the VM running and the pool in place so you can explore them. A
re-run deletes the previous VM and its committed disk first, so every run commits a
fresh VM. Both the re-run and `--cleanup` also remove a `demo-commit-obd` OverlayBD
pool if one exists: qarax serves `--image-ref` VMs from the first OverlayBD pool by
name, so that pool would take over every other demo's OCI VMs.

## Environment variables

| Variable | Default | Description |
|---|---|---|
| `QARAX_SERVER` | `http://localhost:8000` | qarax API URL |
| `QARAX_TOKEN` | `e2e-test-token` | API bearer token |
