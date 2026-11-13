# Test-pruning campaign

Campaign mode prunes one subsystem's whole test surface in one PR: for example the storage stack (storage pools, objects, transfers, OverlayBD), or the sandbox stack. The bars, evidence and validation in [SKILL.md](SKILL.md) and [references/audit.md](references/audit.md) apply to every lane. This file adds the order of work. Each step ends on its completion criterion; do not start the next step early.

## 1. Baseline

Record the subsystem's test and support line counts and every test's pass, fail, skip and flake state at a pinned `origin/master` SHA ([commands](references/audit.md#baseline)). Keep baseline failures, flakes and never-run tests in their own list: they are often product bugs, not stale tests.

Done when every in-scope test has a recorded baseline result.

## 2. Lanes and inventory

Split the surface into **lanes** along production owner boundaries, not file names. Starting points for qarax:

- control-plane HTTP API (`qarax/tests/*.rs`, `qarax/src/handlers/**`)
- control-plane models, scheduling and background monitors (`qarax/src/model/**`, `*_monitor.rs`, `host_deployer.rs`, `secret_provider.rs`)
- node: VMM backends, storage, networking, file transfer (`qarax-node/src/**`)
- shared crates and CLI (`common/`, `cli/`)
- e2e by feature area (`e2e/test_*.py`, `conftest.py`, `helpers.py`)

Include the subsystem's cases in shared harnesses and in e2e.

Done when every test file and e2e test the subsystem owns belongs to exactly one lane.

## 3. Read-only ledger per lane

Give each lane to its own read-only agent. It reads every assigned test in full, including table rows and fixtures, plus the production owners, entry points, callers, history and CI routing. Each test goes into a **ledger** with one mark. A table-driven test or `@pytest.mark.parametrize` is one entry unless its rows need different marks.

- `R`: retain; name the contract and the bug it catches. A retained test that only moves stays `R` with the move noted.
- `F`: retain the contract but repair the test (vacuous assertion, skip on product failure, sleep-based wait, misleading name).
- `C`: consolidate; name the owner that absorbs the assertion first (a sibling table row, a stronger boundary test, the shared crate).
- `D`: delete; name the proof that remains, or why no contract exists.

Judge a test by its assertions, not its name.

Done when every test in the lane has a mark and an evidence line.

## 4. Layer plan per lane

The ledger is input, not the edit list. A second read-only pass looks for the redundant **layer**: the same contract replayed through a mocked collaborator next to a real-boundary test, or the same REST rule asserted in `qarax/tests` and in e2e. Name the **keeper** for each contract. Prefer the real boundary (Axum plus Postgres, a real `qarax-node`) over a mocked collaborator. Correct ledger errors this pass finds.

Done when each lane plan names its retired tests, the keeper per contract, the assertions to carry into keepers, and the test-only production seams unlocked.

## 5. Cutover

Edit lane by lane. Serialize changes to shared harnesses (`spawn_app`, `TestDatabase`, `e2e/conftest.py`, `e2e/helpers.py`) through one owner. With each lane, remove the test-only seams it unlocks: injection parameters, widened visibility, `#[cfg(test)]` hooks, single-impl traits. nextest and pytest discover tests automatically, but check `e2e/run_e2e_tests.sh` and the workflows for anything selected by name. Put durable test-ownership rules in `AGENTS.md`, drawn from mistakes this campaign actually found.

Done when every lane plan is applied and each lane's keepers pass.

## 6. Preservation review

Before claiming completion, have independent reviewers compare deleted coverage against the keepers, one reviewer per boundary group. They look for contracts that lost their only proof, and for new assertions that cannot fail.

For each restored contract, make one deliberate **mutation** of the production owner and confirm the keeper goes red. Then restore the source byte for byte.

Done when every reported gap is restored or rejected with source evidence, and every restored contract has a caught mutation.

## 7. Product defects

A baseline failure or flake that survives into a keeper is a bug report. Fix it at its owner as a separate commit and prove it through the real flow, with a **control** run that reverts the fix and shows the old behaviour. Record unrelated product discrepancies as follow-ups instead of fixing them in the campaign.

Done when each repaired defect has a failing control and a passing candidate on the same harness.

## 8. Reconcile and hand off

When master moved, merge it rather than rebasing a long campaign. When master modified a test the campaign deleted, keep the deletion and port the new contract into the keeper. Rerun locally only the tests the merge touched or CI failed; rerun e2e (`run-e2e` label) when merged changes affect it.

Hand off with the [SKILL.md](SKILL.md#report) report, plus:

- baseline and final test and support line counts, production counted separately;
- lanes, retired layers and keepers;
- preservation gaps found and their mutations;
- product defects with control and candidate proof.
