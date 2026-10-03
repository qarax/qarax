---
name: demo-stabilization
description: Debug and stabilize Qarax demos or bring-up workflows by isolating the failing layer before changing code.
---

# Demo Stabilization

Use this skill when a Qarax demo, example workflow, or multi-VM bring-up script does not complete reliably.

## Goal

Get from "the demo is broken" to a verified end-to-end fix with the fewest blind edits.

## Core approach

1. Reproduce the failure with the real script or workflow.
2. Separate the problem by layer before editing:
   - control-plane / API health
   - node or host health
   - network reachability / relays / bridges
   - guest bootstrap
   - service readiness inside the guest
   - workload behavior
3. Prefer direct evidence over guessing:
   - service logs
   - packet captures
   - guest logs
   - rendered configs
   - exit codes

## Required debugging order

1. Confirm the intended deployment mode.
   - containerized local stack
   - hyperconverged VM
   - host-local bare metal workflow
2. Verify infrastructure first:
   - expected containers or services are up
   - API responds **and accepts the token** (probe an authenticated route like `/hosts`)
   - host kernel matches `/lib/modules` (modules load on demand at VM start)
   - registry and database are reachable if required
3. Inspect service logs before source code.
4. Check the network path if anything is unreachable:
   - listener exists
   - bridge/tap/TUN exists
   - relay process is bound correctly
   - packets actually arrive and return
5. Inspect guest state only after infrastructure is known-good:
   - cloud-init or first-boot logs
   - systemd status
   - `journalctl`
   - `ss -ltnp`
   - firewall/ruleset state
6. Change only the layer that is actually failing.
7. Re-run end to end and do not stop at partial progress.

## Good evidence to gather

- final script output and exit code
- service logs from `qarax`, `qarax-node`, and related containers
- API responses from `curl` or `qarax ... -o json`
- bridge/tap packet captures when traffic disappears
- guest bootstrap logs and readiness markers
- rendered config files and generated kubeconfig/user-data where relevant

## Common repo-specific failure classes

- stack binary/API mismatch after code changes
- auth: 401 from a missing/wrong `QARAX_TOKEN` (the local stack enables token auth; `/` is public so it can't detect this). Use `require_server` from `demos/lib.sh`
- API contract drift: demo sends a field or flag the API/CLI removed (422 / clap usage error, which `set -e` + `$(...)` can turn into a silent exit)
- host kernel modules unavailable (`uname -r` has no `/lib/modules` dir after an unrebooted kernel upgrade): TCMU/OverlayBD and iptables failures at VM start
- stale host records or wrong host selection in E2E-style environments
- relay or bridge source-address mismatches
- guest networking configured but not actually applied
- image pull failures due to registry trust or mirror config
- missing product surface in the CLI for something the demo needs

## When to patch product code

Only patch Rust or CLI code when the demo failure exposes a missing or incorrect product capability. Otherwise, prefer fixing the demo assets, cloud-init, or environment wiring.

## Validation

- For shell/demo changes: syntax-check edited scripts and rerun the full workflow.
- For Rust changes: run `make lint` and appropriate tests.
- Do not declare success until the real demo or workflow completes with the expected proof.
