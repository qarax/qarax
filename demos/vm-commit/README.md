# VM Commit Demo

Demonstrates the `vm commit` workflow: converting an OCI image-backed VM
(OverlayBD) into a standalone raw-disk VM.

## What it does

1. Ensures the qarax stack is running (starts it via `./hack/run-local.sh` if not)
2. Uses an UP host, or registers and initialises qarax-node if there is none
3. Creates an OverlayBD storage pool backed by the local test registry
4. Creates a Local storage pool for the committed raw disk
5. Attaches both pools to the host
6. Pushes `busybox:latest` to the local registry
7. Creates a VM with `--image-ref` pointing at the pushed image (async job)
8. Runs `vm commit` to byte-copy the OverlayBD block device to a raw disk
9. Verifies that `image_ref` is cleared and the committed disk object exists

## Prerequisites

- Linux host with `/dev/kvm` (KVM virtualisation)
- Docker (the demo pushes to the stack's local registry on `localhost:5001`)
- `jq`, `curl`
- `qarax` CLI (built automatically with `cargo` if missing)

If your server uses a token other than the local default, export `QARAX_TOKEN`.

## Usage

```bash
# Start the stack if not running, then run the demo
./demos/vm-commit/run.sh

# Custom server URL
QARAX_SERVER=http://localhost:8000 ./demos/vm-commit/run.sh

# Tear down the VM, storage objects and pools left by a previous run
./demos/vm-commit/run.sh --cleanup
```

The demo leaves its resources in place so you can explore them, and it is safe
to re-run: existing pools and the VM are reused, and the commit step is skipped
if the VM was already committed.

## Environment variables

| Variable | Default | Description |
|---|---|---|
| `QARAX_SERVER` | `http://localhost:8000` | qarax API URL |
| `QARAX_TOKEN` | `e2e-test-token` | API bearer token |
| `REGISTRY_PUSH_URL` | `localhost:5001` | Registry URL for docker push (host-side) |
| `REGISTRY_INTERNAL_URL` | `registry:5000` | Registry URL as seen inside Docker network |
| `QARAX_NODE_ADDRESS` | `qarax-node` | Node hostname (inside Docker network) |
| `QARAX_NODE_PORT` | `50051` | Node gRPC port |
