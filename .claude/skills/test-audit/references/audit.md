# Audit reference

Use after the [authoring gate](../SKILL.md#authoring-gate). Whole-subsystem campaigns also follow [CAMPAIGN.md](../CAMPAIGN.md).

## Junk patterns

Reject new matches unless the [retention bar](../SKILL.md#retention-bar) names an independent contract. Audit existing matches.

**Proves nothing**

- No assertion, or only `.unwrap()` on a call whose failure is impossible here.
- Self-comparison, or the expected value comes from the code under test (`assert_eq!(f(x), f(x))`, building the expected struct with the same helper).
- `serde` round-trips of a struct with derived impls (serialize then deserialize the same value). Keep them only when they pin a wire or DB string (`#[serde(rename_all)]`, `sqlx(type_name)`) against a literal.
- `Default` or constant tests that copy the literal from the source. Keep user-facing defaults (`configuration/*.yaml`, API request defaults).

**Duplicates proof**

- A shared helper tested again in each crate that calls it (e.g. `common::cpu_list` replayed in `qarax` and `qarax-node`). The owner crate keeps the table; callers keep one test only if they add behaviour.
- Private helper or call-shape tests duplicating an HTTP integration or e2e test of the same contract.
- The same contract in `qarax/tests/*.rs` and in `e2e/` with no distinct transport or lifecycle risk. The Rust test usually owns REST semantics; e2e owns the real VM, gRPC and SDK path.
- Copied setup: `spawn_app`, `configure_database`, `TestDatabase` and `Drop` cleanup duplicated per file. Consolidate into one shared module.

**Couples to implementation**

- Asserting SQL text, log lines, tracing spans or internal call order instead of results.
- Source greps and copied inventories (route lists, enum variant lists, file lists).
- Tests that break on an identifier-only rename.

**Test-only seams**

- Items that are `pub`, `pub(crate)`, `#[cfg(test)]`, `#[doc(hidden)]` or feature-gated only so tests can reach them.
- Traits or generic parameters with one production impl plus a test impl, injected only for tests.
- Mocks (fake `VmmManager`, fake gRPC client) that implement the behaviour being asserted, or pretend two distinct APIs behave alike.

**Cannot fail**

- `pytest.skip` after waiting for RUNNING, READY or a route: the regression becomes a skip and CI stays green.
- Or-assertions that admit the failure state (`is_success() || is_redirection()` where only one is correct, `status in (200, 404)`).
- Negative tests whose rejection comes from an unrelated guard (a 404 for a missing parent rather than the validation being named), or that production can never reach.
- `try/except: pass` or `let _ =` around the step that proves the behaviour.
- Fixtures that fabricate state the owner should produce: rows inserted directly where the API or monitor should write them, then asserted as if production wrote them.

**Misleading**

- The test name promises behaviour its inputs or assertions never exercise. Judge by assertions, not names.

## Health checks

- **Never runs**: e2e tests skipped in every recent master run; env-gated tests (`ENABLE_BOOTC_UPGRADE_E2E`, `ENABLE_OTEL`) whose gate CI never sets; `#[ignore]`. Check several runs, not one.
- **Flaky**: grep recent master logs for `FLAKY` and `TRY 2`. nextest retries once, so a flake never turns CI red.
- **Timeout-dominated**: per-test durations clustering near a timeout. Find what waits: pool or connection timeouts, monitors, server shutdown, `Drop` cleanup, gRPC connect (5 s). A fast test can be slow only because of its harness.
- **E2E hygiene**: fixed `time.sleep` waits, cleanup that swallows errors and leaks VMs into later tests, ordering dependencies between tests. `e2e/` is not linted in CI (`make ruff-check` only covers `python-sdk/`).

## Baseline

Pin `git fetch origin master && git rev-parse origin/master`. Use the latest green master runs; CI runs the whole suite so you need not rebuild the workspace locally.

```bash
gh run list --repo qarax/qarax --branch master --workflow rust-ci.yml --limit 5 \
  --json databaseId,conclusion,headSha,displayTitle
gh run view <run-id> --repo qarax/qarax --json jobs --jq '.jobs[] | [.name, .conclusion, .databaseId] | @tsv'

# Rust (job "CI Checks"): summary, slow, flaky, per-test durations
gh run view --repo qarax/qarax --job <check-job-id> --log | sed -E 's/\x1b\[[0-9;]*m//g' > ci.log
grep -E 'Summary \[|SLOW|FLAKY|TRY [0-9]' ci.log
grep -oE 'PASS \[ *[0-9.]+s\].*' ci.log | sed -E 's/PASS \[ *([0-9.]+)s\] *(\([^)]*\) *)?/\1 /' | sort -rn | head -30

# E2E (job "E2E Tests")
gh run view --repo qarax/qarax --job <e2e-job-id> --log | grep -E ' (SKIPPED|FAILED|ERROR)|[0-9]+ passed'
```

## Candidate evidence

Before editing, read the full test, its production owner, entry points, callers, callees, sibling tests, overlapping proof in other layers, and history (`git log -L` or `git log -S`). For claims about a dependency (sqlx, tonic, Cloud Hypervisor SDK), read its source.

Record for each candidate:

- Exact location and what failure it can detect.
- Non-test callers of any seam it uses.
- The stronger proof that remains, or why none is needed.
- Why it exists (commit, PR) and what deleting it unlocks.
- Risk and the focused validation command.

No evidence, no deletion. Delete obsolete seams and dead paths with their tests; move retained regressions to the owner. Prefer net-negative production LOC. Never add replacement implementation tests or turn uncertainty into cleanup.

## Validation

Run every build and test through `capped`, one at a time, on the changed packages only.

```bash
pg_isready -h 127.0.0.1 -p 5432          # qarax tests need Postgres; otherwise ask before `make test-deps`
capped cargo nextest run -p qarax --test vms -E 'test(name)'
capped cargo nextest run -p qarax --lib -E 'test(name)'
capped cargo nextest run -p qarax-node --lib
capped make lint                          # clippy -D warnings, whole workspace
capped cargo +nightly fmt --all -- --check
git diff --check && git diff --numstat
```

- CI tests with `--features qarax/otel,qarax-node/otel`; add them when timing or tracing matters.
- Clippy flags private dead code after a deletion, not `pub` items: grep for remaining callers of anything a deleted test used. Check `[dev-dependencies]` (`once_cell`, `tempfile`) and `e2e/helpers.py` / `conftest.py` fixtures for orphans.
- E2E changes: `cd e2e && uv run --with ruff ruff check .`, then `./run_e2e_tests.sh -k <expr>` only with the user's go-ahead (Docker, KVM). Release binaries are reused if present; rebuild after code changes.
- For each keeper that absorbed coverage, mutate the production owner once, confirm the keeper goes red, then restore it byte for byte.
- Changed SQL: `cargo sqlx prepare --workspace`. Never edit `openapi.yaml` or `python-sdk/` by hand.
- Finish with `/code-review` on the diff.
