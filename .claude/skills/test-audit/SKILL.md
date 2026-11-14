---
name: test-audit
description: Gate new or changed tests and audit existing ones in qarax (Rust nextest suites, Postgres-backed integration tests, e2e pytest). Rejects duplicate or implementation-coupled proof, flags tests that cannot fail, never run or hide flakes, and removes test-only production seams. Use when writing, changing, reviewing, pruning or auditing tests.
---

# Test Audit

Adapted for qarax from OpenClaw's `test-audit` skill (MIT, Copyright (c) 2026 OpenClaw Foundation).

- **Authoring**: the gate below applies to every test you add or change.
- **Audit**: find low-value, broken or unhealthy proof, report it, then fix in coherent batches per production owner. Never chase deletion counts.
- **Campaign** (one subsystem's whole test surface in one PR): read [CAMPAIGN.md](CAMPAIGN.md) first.

## Test layers

Every contract has one primary owner: the strongest boundary that runs on every PR.

| Layer | Where | Runs | Owns |
|---|---|---|---|
| Pure unit | `#[cfg(test)] mod tests` in `*/src/**` | every PR (CI `Test` job) | parsing, planning, pure mapping (cpu lists, disk names, cloud-init, VMM config) |
| DB-backed unit | `TestDatabase` helpers, e.g. `qarax/src/model/hosts.rs`, `qarax/src/ha_monitor.rs` | every PR, needs Postgres | SQL semantics, placement queries, monitor decisions over real rows |
| HTTP integration | `qarax/tests/*.rs` (`spawn_app`: real Axum server, fresh Postgres DB) | every PR, needs Postgres | REST contract: status codes, error bodies, auth, persistence through the API, behaviour with an unreachable node |
| Node unit | `qarax-node/src/**` tests | every PR | VMM config building, preflight, file transfer, sysfs parsing over temp dirs |
| E2E | `e2e/test_*.py` via `e2e/run_e2e_tests.sh` | push to master, and PRs labelled `run-e2e` | anything needing KVM, a real hypervisor, guest networking or control plane to node gRPC |

- A contract proven only in e2e has no PR-time proof. Moving proof from Rust into e2e trades PR coverage for realism; say so.
- A second layer earns a test only for a distinct risk: transport (gRPC, HTTP, generated SDK), lifecycle (real process, real VM) or persistence.
- `demos/` are not tests; CI never runs them.

## Authoring gate

Before adding a test, answer:

1. Which observable behaviour, invariant or independent contract does it protect?
2. Which credible regression makes it fail?
3. Why does existing proof miss it? Search the owner's siblings, the other layers above, and e2e. Extend a table or shared helper instead of adding a near-copy; reuse setup instead of copying `spawn_app` or `TestDatabase`.
4. Does it need a `pub` item, `#[cfg(test)]` hook, trait, injection parameter or feature that has no production caller? Prove it at the real boundary instead.

- A regression test must fail before the fix for the intended reason and pass after it. If you cannot show the pre-fix failure, say so.
- One regression test at the owner covers a bug; do not replay it at every layer the bug crosses.
- Behaviour-preserving refactors must not break the test.
- E2E waits poll with a deadline (`wait_for_status` in `e2e/helpers.py`), never `time.sleep` for a state. Never `pytest.skip` on a product failure.

## Junk patterns

Full checklist with Rust and pytest examples: [references/audit.md](references/audit.md#junk-patterns).

- No assertions, self-comparisons, expected values computed by the code under test.
- Copied inventories that only mirror source (route lists, enum variants, internal constants).
- Private-helper or call-shape tests that duplicate a boundary test; one shared helper tested again in every crate that calls it.
- Test-only seams; mocks that implement the behaviour being asserted.
- Tests that cannot fail: skips on product failure, rejections production never reaches, or-assertions that accept the failure state.
- Names promising behaviour that the inputs or assertions never exercise.

## Retention bar

Keep these even when slow or static:

- REST status codes and error shapes (the CLI and Python SDK depend on them), auth, credential handling (`credential_ref`).
- Migrations, persisted enum strings, data written by older versions.
- `proto/node.proto` semantics: control plane and nodes upgrade separately.
- Config defaults in `configuration/*.yaml`, CLI flags and output that scripts parse.
- Scheduling and placement invariants (capacity, NUMA, GPU, HA), storage ownership and cleanup (data loss), observable ordering.

Slow or static is not a reason to delete; make it cheaper. A baseline failure or flake may be a product bug: reproduce it and fix the owner.

## Health

Audits also report proof that exists but does not protect anything ([details](references/audit.md#health-checks)):

- **Never runs**: skipped in every recent CI run, gated on env vars CI never sets, `#[ignore]`.
- **Flaky**: nextest `retries = 1` (`.config/nextest.toml`) turns a flaky test green; only `FLAKY`/`TRY 2` lines in the CI log show it.
- **Timeout-dominated**: duration close to a timeout (nextest's 60 s slow period, gRPC 5 s connect). A cluster of identical durations means something waits for a timeout.

## Audit workflow

1. **Baseline** at a pinned `origin/master` SHA from recent green CI runs ([commands](references/audit.md#baseline)).
2. **Discovery is read-only.** Mark every test in scope R/F/C/D (see [CAMPAIGN.md](CAMPAIGN.md#3-read-only-ledger-per-lane)) with [candidate evidence](references/audit.md#candidate-evidence). Report before editing; prefer a few high-confidence candidates.
3. **Edit** one owner batch at a time. Delete the test-only seams that batch unlocks.
4. **Validate** ([commands](references/audit.md#validation)).

## Report

- Baseline: SHA, test counts per layer, failures, flakes, slow and never-run tests.
- Each candidate: mark, location, contract, remaining proof, evidence, risk, validation command.
- Health findings separately from pruning candidates.
- Retained false positives: looked like junk, kept, and why.
- Production versus test LOC deltas (`git diff --numstat`).
- Follow-ups: uncovered contracts and product bugs found. Do not fix them inside the audit.
- Commit or push only when asked; push to `upstream`.
