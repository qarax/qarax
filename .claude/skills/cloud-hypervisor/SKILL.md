---
name: Cloud Hypervisor Reference
description: Consult upstream Cloud Hypervisor capabilities and implementation details.
---

# Cloud Hypervisor Reference

Use this skill when working on VM behavior, device support, configuration options, or Cloud Hypervisor API behavior in `qarax-node`.

## Source of truth

- Repository: https://github.com/cloud-hypervisor/cloud-hypervisor

## Guidance

1. Prefer upstream docs and source in the Cloud Hypervisor repository for capability checks and behavior details.
2. Check upstream at the version qarax pins (`versions/cloud-hypervisor-version`; SDK tag in `qarax-node/Cargo.toml`), not `main`, before changing manager/service logic.
3. When behavior in Qarax differs from upstream, document the rationale in code comments or PR notes.
