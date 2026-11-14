# Local Stack Scripts

`hack/run-local.sh` runs the qarax stack locally in Docker: the control plane,
qarax-node with KVM passthrough, Postgres and an OCI registry. It is the compose
stack the e2e tests use (`e2e/docker-compose.yml`, project `e2e`).

## Quick Start

```bash
make run-local        # build, start, register host local-node, create overlaybd-pool
make run-local VM=1   # same, but the node is a bootc microVM (see below)
make stop-local       # remove the stack and its volumes
```

Re-running `make run-local` is cheap: cargo and the image builds are no-ops when
nothing changed, and Compose only recreates containers whose image or config
changed, so running VMs survive.

Then try the CLI or a demo (`demos/README.md`):

```bash
export QARAX_TOKEN=e2e-test-token
qarax vm create --name alpine --image-ref public.ecr.aws/docker/library/alpine:latest
qarax vm start alpine
./demos/vm-exec/run.sh
```

The default stack runs one node. Two-host demos start the second node
container (`qarax-node-2`, compose profile `multi-node`) themselves.

## Environment Variables

- `SKIP_BUILD=1`: use the existing release binaries instead of running cargo.
- `REBUILD=1`: rebuild the Docker images without cache.
- `QARAX_TOKEN` / `QARAX_TEST_TOKEN`: API token (default `e2e-test-token`).

## VM mode

`make run-local VM=1` runs the node the way a real host does: the appliance
image installed with bootc, booted as a Cloud Hypervisor microVM (compose
service `bootc-vm`, the node the e2e upgrade tests deploy to), registered as
host `local-node-vm`. It needs `qemu-img` and nested virtualization, not sudo.
See [docs/DEVELOPMENT.md](../docs/DEVELOPMENT.md#the-bootc-node-microvm).

## Networking

qarax-node creates a TAP device per NIC, named `qt<vm-id-prefix>n<nic-index>`
(e.g. `qt24b6061en0`), and deletes it with the VM:

```bash
docker compose -f e2e/docker-compose.yml exec qarax-node ip link show type tun
```

The node container uses the nft backend of iptables, so firewall rules don't
depend on the host's legacy `ip_tables` module.

## Common Issues

### `/dev/net/tun` not found

```bash
sudo modprobe tun
sudo mkdir -p /dev/net
sudo mknod /dev/net/tun c 10 200
sudo chmod 0666 /dev/net/tun
```

Then recreate the node: `make stop-local && make run-local`.

### Kernel modules fail to load

qarax-node loads host modules such as `target_core_user` and `tcm_loop` on
demand. If `uname -r` has no matching `/lib/modules/` directory (the kernel was
upgraded without a reboot), anything not already loaded fails. Reboot.

### VM won't start

```bash
docker compose -f e2e/docker-compose.yml logs qarax-node
qarax host list
qarax vm get <vm>
```

### Services won't start

```bash
make stop-local
REBUILD=1 make run-local
```

## API Documentation

- Swagger UI: http://localhost:8000/swagger-ui
- OpenAPI JSON: http://localhost:8000/api-docs/openapi.json
