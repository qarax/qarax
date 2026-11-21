# Installing qarax

This guide sets up a release of qarax on real hosts: the control plane, its
database, and the nodes that run VMs. For a local stack on one machine, use
`make run-local` instead (see the README).

## What you run

- **The control plane**, `qarax-server`: the REST API (port 8000) and the
  background tasks that schedule, monitor and fail over VMs. It keeps its
  state in PostgreSQL 16 or later. Run one instance.
- **Nodes**: hosts that run VMs. Each runs the appliance image, a bootc image
  with `qarax-node` (gRPC, port 50051), Cloud Hypervisor, Firecracker and
  OverlayBD. The control plane installs and upgrades it over SSH.
- **The CLI**, `qarax`, wherever you manage qarax from.

Each release `vX.Y.Z` publishes:

| Artifact | Where |
|---|---|
| `qarax-vX.Y.Z-x86_64-linux-musl.tar.gz`: the four static binaries (`bin/`), `configuration/`, `gen-node-tls.sh`, the systemd units and the changelog | GitHub release `vX.Y.Z`, with `SHA256SUMS` |
| Control-plane image | `ghcr.io/qarax/qarax:vX.Y.Z` |
| Node appliance image | `ghcr.io/qarax/qarax-vmm-host:vX.Y.Z` |

Both images are signed with cosign (keyless, from the release workflow).

## 1. Certificates

The control plane and the nodes authenticate each other with mutual TLS. Make
a CA, the control plane's client certificate, and a certificate for each node
address you will register (a DNS name or an IP), on a machine you trust:

```bash
./gen-node-tls.sh ./node-tls 10.0.0.42 10.0.0.43
```

`node-tls/control-plane/` goes to the control plane (step 3), and
`node-tls/nodes/ADDRESS/` to each node (step 4). Keep `node-tls/ca-key.pem`
off the hosts. To add a node later, run the script again with its address:
it reuses the CA.

## 2. Database

Create a PostgreSQL 16+ database and a user that owns it. The control plane
runs its migrations at startup; there are no down migrations.

## 3. Control plane

The control plane reads `configuration/base.yaml` and `production.yaml`
(`APP_ENVIRONMENT=production`, which both the image and the unit set), and
these environment variables:

| Variable | |
|---|---|
| `DATABASE_HOST`, `DATABASE_PORT`, `DATABASE_USERNAME`, `DATABASE_PASSWORD`, `DATABASE_NAME` | The database |
| `AUTH_TOKENS` | Comma-separated API tokens. Production enables token auth with none, so set at least one |
| `NODE_TLS_DIR` | The control plane's node certificates (default `/etc/qarax/node-tls`) |
| `VM_KERNEL`, `VM_INITRAMFS`, `VM_CMDLINE`, `VM_FIRMWARE` | Boot defaults for new VMs, as paths on the nodes. The appliance ships the guest kernel as `/var/lib/qarax/images/vmlinux` and the UEFI firmware as `/usr/share/cloud-hypervisor/CLOUDHV.fd` |

### In a container

```bash
docker run -d --name qarax -p 8000:8000 \
  -e DATABASE_HOST=db.example.com -e DATABASE_PASSWORD="$DB_PASSWORD" \
  -e AUTH_TOKENS="$QARAX_TOKEN" \
  -v "$PWD/node-tls/control-plane:/etc/qarax/node-tls:ro" \
  -v qarax-ssh:/root/.ssh \
  ghcr.io/qarax/qarax:vX.Y.Z
```

The `/root/.ssh` volume keeps the SSH host keys the control plane pins when it
deploys hosts. Database backups are written into a local storage pool's path
inside the container, so mount a volume there if you use them.

### Or as a systemd service

From the release tarball:

```bash
tar -xzf qarax-vX.Y.Z-x86_64-linux-musl.tar.gz && cd qarax-vX.Y.Z-x86_64-linux-musl
useradd --system --home-dir /var/lib/qarax --shell /usr/sbin/nologin qarax
install -m 0755 bin/qarax-server /usr/local/bin/
install -d /etc/qarax && cp -r configuration /etc/qarax/
install -d /usr/share/qarax && cp -r ui /usr/share/qarax/   # the web UI
install -d -m 0750 -g qarax /etc/qarax/node-tls
install -m 0640 -g qarax ../node-tls/control-plane/* /etc/qarax/node-tls/
install -m 0640 -g qarax /dev/null /etc/qarax/qarax.env   # then fill in the variables above
install -m 0644 qarax.service /etc/systemd/system/
systemctl daemon-reload && systemctl enable --now qarax
```

The unit runs as `qarax`, with its home (and so the pinned SSH host keys) in
`/var/lib/qarax`.

Check it: `curl http://localhost:8000/` answers `hello`, and
`curl -H "Authorization: Bearer $QARAX_TOKEN" http://localhost:8000/v1/hosts`
returns an empty page. The API reference is at `/swagger-ui`, and the web UI
at `/ui` (the image has it; elsewhere `qarax-server` serves it from `ui_dir`,
`UI_DIR`, by default `/usr/share/qarax/ui`).

## 4. Nodes

A node needs x86_64 with KVM and a bootc-capable OS (Fedora or CentOS Stream
bootc), reachable over SSH from the control plane. Point the CLI at the
control plane (`qarax configure --server http://CONTROL-PLANE:8000`, token in
`QARAX_TOKEN` or `--token`), then for each node:

```bash
qarax host add --name node-01 --address 10.0.0.42 --user root
qarax host deploy node-01 --image ghcr.io/qarax/qarax-vmm-host:vX.Y.Z --ssh-key ~/.ssh/id_ed25519

# Once it has rebooted into the appliance, install its certificates:
ssh root@10.0.0.42 'install -d -m 700 /etc/qarax-node/tls'
scp node-tls/nodes/10.0.0.42/* root@10.0.0.42:/etc/qarax-node/tls/

qarax host init node-01
qarax host list    # node-01 is up
```

The `--address` must be the one the node's certificate was issued for.
`qarax-node` restarts every few seconds until its certificates are in place.
For SSH passwords kept outside qarax, see "Host credentials" in the README.

## 5. A first VM

VMs need somewhere to put their disks and a network to attach to. Create a
storage pool and a network (see the [CLI README](../cli/README.md)), then:

```bash
qarax vm create --name hello --vcpus 1 --memory 512MiB --image-ref public.ecr.aws/docker/library/alpine:latest
qarax vm start hello
```

The demos in [demos/](../demos/) walk through the main features against a
running stack.

## Upgrading

Read the new release's section of [CHANGELOG.md](../CHANGELOG.md) first:
before 1.0, a minor release can change the API, the node protocol and the
database in ways that need action.

1. Back up the database (`pg_dump`, or `qarax backup create database --pool POOL`).
   Migrations only go forward, so going back means restoring this backup with
   the previous release.
2. Upgrade the control plane: the new image, or the new `qarax-server` and
   `configuration/`. It migrates the database when it starts.
3. Upgrade each node to the same release: `qarax host deploy NODE --image
   ghcr.io/qarax/qarax-vmm-host:vX.Y.Z`. `/etc/qarax-node/tls` survives the
   switch. The control plane and the nodes must run the same release; the
   protocol between them isn't kept compatible across releases.
