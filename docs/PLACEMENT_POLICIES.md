# Placement Policy Guide

This guide explains Qarax host placement metadata and VM placement policies:

- **host reservation classes** for coarse-grained pool selection
- **host placement labels** for exact key/value matching
- **VM placement policies** for reservation, affinity, anti-affinity, and spread

These controls sit on top of the existing scheduler checks for host state,
architecture, networks, storage-pool locality, GPU requirements, and capacity.

## Mental model

There are three inputs:

- a **host** may advertise:
  - `reservation_class` like `general`, `reserved`, or `batch`
  - `placement_labels` like `zone=west`, `rack=r1`, `tenant=blue`
- a **VM** may carry **tags**
- a **VM create request** may include a **placement policy**

In practice:

- use **reservation classes** to keep broad categories of workloads on a subset
  of hosts
- use **placement labels** when you need exact host traits such as zone, rack, or
  hardware generation
- use **VM tags** to define which workloads are considered peers for affinity,
  anti-affinity, and spread

## What the scheduler always enforces

Before Qarax considers placement-policy rules, a host still has to be otherwise
eligible:

- host status must be `up`
- hosts in `maintenance` are excluded from new placement
- architecture must match when requested
- required managed networks must be attached to the host
- required storage-pool locality must be satisfied
- GPU filters must match when requested
- CPU, memory, disk headroom, and memory-health-floor checks must pass

Placement policy narrows or orders the set of already-eligible hosts. It does
not bypass the base scheduler checks.

## How policy evaluation works

Qarax evaluates placement policy in two phases.

### 1. Hard filters

These rules remove hosts entirely:

- `reservation_class`: host must have an exact matching reservation class
- `required_host_labels`: host must contain **all** requested key/value labels
- `anti_affinity_tags`: host is excluded if it already runs any active VM whose
  tags overlap with any listed tag

For anti-affinity, Qarax checks active VMs on the candidate host. "Active" here
means VM status is not `shutdown` and not `unknown`.

### 2. Preferences / scoring

The remaining hosts are ordered by:

1. `preferred_host_labels`: hosts matching **all** preferred labels sort ahead
2. `affinity_tags`: hosts with more active VMs containing **all** listed tags
   sort ahead
3. `spread_tags`: hosts with fewer active VMs containing **all** listed tags
   sort ahead
4. lower host `load_average`
5. host name as a stable tie-breaker

That means:

- **affinity** is a soft preference, not a hard requirement
- **spread** is a soft preference, not a hard guarantee
- if no host matches an affinity or preferred-label hint, Qarax falls back to
  the next score dimension instead of failing

## What counts as a "peer" VM

Affinity and spread do not look at the VM being created. They look at **existing
active VMs already running on candidate hosts**.

Use the same tag set on each replica and point the policy at those same tags.

Example:

- create each web replica with `--tag app=web`
- use `--spread-tag app=web` to push later replicas toward the least populated
  host

## Host-side metadata

### Add a host with reservation class and labels

```bash
qarax host add \
  --name node-west-1 \
  --address 192.168.1.21 \
  --user root \
  --reservation-class reserved \
  --label zone=west \
  --label rack=r1
```

### Update host placement metadata later

```bash
qarax host placement set node-west-1 \
  --reservation-class reserved \
  --label zone=west \
  --label rack=r2
```

### Clear placement metadata

```bash
qarax host placement set node-west-1 --clear-reservation-class --clear-labels
```

### API shape

Host placement metadata is updated with:

```http
PUT /hosts/{host_id}/placement
Content-Type: application/json
```

```json
{
  "reservation_class": "reserved",
  "placement_labels": {
    "zone": "west",
    "rack": "r1"
  }
}
```

## VM-side policy

### CLI flags

`qarax vm create` supports:

- `--reservation-class <name>`
- `--require-host-label key=value` (repeatable)
- `--prefer-host-label key=value` (repeatable)
- `--affinity-tag <tag>` (repeatable)
- `--anti-affinity-tag <tag>` (repeatable)
- `--spread-tag <tag>` (repeatable)
- `--tag <tag>` to label the VM itself

### API shape

`POST /vms` accepts:

```json
{
  "name": "api-2",
  "tags": ["app=api", "tier=frontend"],
  "hypervisor": "cloud_hv",
  "boot_vcpus": 2,
  "max_vcpus": 2,
  "memory_size": 2147483648,
  "placement_policy": {
    "reservation_class": "reserved",
    "required_host_labels": {
      "zone": "west"
    },
    "preferred_host_labels": {
      "rack": "r1"
    },
    "affinity_tags": ["app=api"],
    "anti_affinity_tags": ["role=db-primary"],
    "spread_tags": ["app=api"]
  }
}
```

The policy is persisted with the VM so Qarax can reuse it for later scheduling
decisions such as host evacuation.

## Common workflows

### Reserve a class of hosts for sensitive workloads

Hosts:

```bash
qarax host placement set node-a --reservation-class reserved
qarax host placement set node-b --reservation-class reserved
qarax host placement set node-c --reservation-class general
```

VM:

```bash
qarax vm create \
  --name payroll-api \
  --vcpus 2 \
  --memory 2GiB \
  --reservation-class reserved
```

Result: `payroll-api` is only considered for `node-a` and `node-b`.

### Keep replicas in one zone, but avoid colocation

```bash
qarax vm create \
  --name api-1 \
  --tag app=api \
  --vcpus 2 \
  --memory 2GiB \
  --require-host-label zone=west \
  --anti-affinity-tag app=api
```

Result:

- only hosts with `zone=west` are eligible
- hosts already running an active VM tagged `app=api` are excluded

### Prefer a rack, but fall back if needed

```bash
qarax vm create \
  --name cache-1 \
  --tag app=cache \
  --vcpus 2 \
  --memory 2GiB \
  --prefer-host-label rack=r1
```

Result: Qarax prefers `rack=r1`, but it will still place the VM elsewhere if no
eligible `rack=r1` host exists.

### Spread replicas across hosts

```bash
qarax vm create \
  --name web-1 \
  --tag app=web \
  --vcpus 2 \
  --memory 2GiB \
  --spread-tag app=web
```

Create every `web-*` replica with the same `--tag app=web` and `--spread-tag
app=web`.

Result:

- the first replica falls back to normal scheduler ordering because no peers
  exist yet
- later replicas prefer hosts with fewer active `app=web` VMs

### Prefer colocation with an existing workload

```bash
qarax vm create \
  --name worker-1 \
  --tag app=worker \
  --vcpus 2 \
  --memory 2GiB \
  --affinity-tag app=worker
```

Result: new workers tend to land on hosts already running active `app=worker`
VMs, unless harder filters or lower-level eligibility checks rule those hosts
out.

## Maintenance and evacuation behavior

Maintenance behavior stays strict:

- placing a host into `maintenance` keeps it manageable
- that host is excluded from new scheduling
- host evacuation leaves the source host in `maintenance`

During evacuation, Qarax rebuilds the VM's scheduling request from persisted VM
state, including:

- architecture
- attached managed networks
- the VM's placement policy

That means reservation, required labels, anti-affinity, affinity, and spread are
all reused when Qarax looks for an evacuation target.

## Troubleshooting

If VM creation returns a "no eligible host" style `422`, check the policy from
the outside in:

1. inspect host state: `qarax host list`
2. inspect host metadata: `qarax host get <host>`
3. verify the VM tags you are using for peer matching are consistent
4. verify reservation classes and label values are exact matches
5. verify enough non-maintenance hosts remain after anti-affinity filters
6. verify the usual scheduler constraints still pass (resources, storage,
   networks, architecture, GPUs)

For JSON output:

```bash
qarax host get node-a -o json
qarax vm get my-vm -o json
```

## Current limits

Current placement policy is intentionally simple:

- no weighted scoring knobs
- no explicit "must spread across N hosts" guarantee
- no separate affinity group resource; VM tags are the grouping mechanism
- no policy-specific preemption or reservations beyond exact reservation-class
  matching
- no maintenance override; maintenance always excludes new placement
