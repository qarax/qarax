---
name: ci-debug
description: Debug failing Qarax CI jobs by identifying the first real error, reproducing locally, and validating the fix.
---

# CI Debug

Use this skill when a GitHub Actions workflow, PR check, or other CI run is failing for this repository.

## Goal

Get from "CI is red" to a verified root-cause fix without guessing from secondary failures.

## Core approach

1. Identify the exact failing workflow, job, and step.
2. Read the full failing log and capture the first actionable error, not the last cascade.
3. Separate infrastructure or flake issues from real product regressions before editing code.
4. Reproduce the smallest failing step locally with existing repo commands.
5. Fix the root cause in one coherent pass, then rerun the relevant validation.
6. If a fix is pushed with authorization, watch the replacement run to completion; do not report green while another required job is pending.

## Required debugging order

1. Record the failing context:
   - workflow name
   - job name
   - commit or PR SHA
   - failing step
2. Inspect logs before source code:
   - note the first compile error, test failure, or service startup error
   - ignore downstream failures until the primary error is understood
3. Check for non-code causes:
   - missing or unhealthy services
   - registry or network failures
   - Docker or database startup problems
   - timeouts or obvious flakes
4. Read `ci/main.go` before reproducing a Dagger failure. Match all inputs that can change compilation or lint behavior:
   - workspace features (CI currently enables `qarax/otel,qarax-node/otel`)
   - target (`x86_64-unknown-linux-musl` via repo Cargo configuration)
   - `SQLX_OFFLINE`
   - Rust and Clippy version shown in the failing log
   - regeneration performed inside the failing stage
5. Reproduce locally using existing repo commands:
   - `make fmt`
   - `make lint`
   - `make build`
   - `make test`
   - narrower `cargo build -p ...` or `cargo nextest run -p ...` commands when appropriate
6. For SQL query changes, run `cargo sqlx prepare --workspace`.
7. For user-facing changes, verify CLI and E2E impact as needed.
8. Prefer fixing all required fields, arguments, or call sites together after reviewing full signatures and interfaces.
9. Validate with the same or stricter checks than CI before considering the issue fixed.

## Dependency and generated-code failures

- After upgrading `tonic`, `prost`, `reqwest`, or other HTTP/gRPC dependencies, run the OTEL-enabled workspace build and Clippy command from `ci/main.go`; default-feature builds do not exercise Qarax's telemetry layers.
- Run `cargo audit` and inspect reverse dependency trees for every vulnerable version. Confirm the vulnerable package is absent rather than assuming a lockfile update removed every path.
- CI's `rust:1` image follows current stable Rust. If a lint appears only in CI, reproduce with that exact Rust version and install its musl target before changing code.
- Never edit tonic/prost output under `target/.../out`. If a lint originates entirely in generated protobuf code, place the narrowest justified lint allowance on the module containing `tonic::include_proto!`.
- Do not apply generated-code allowances to handwritten helpers automatically. Inspect each remaining warning and scope any necessary allowance to the individual gRPC boundary.

## Python SDK failures

- `python-sdk/qarax-api-client/` is generated; regenerate it instead of editing it directly.
- `python-sdk/examples/` is maintained source and may be edited directly.
- CI regenerates the SDK and then runs Ruff on the raw output. Reproduce the exact `PythonSdkLint` commands in `ci/main.go`; `make sdk` additionally formats/fixes output and is not identical.
- Regeneration used only for verification can dirty generated files because of formatting differences. Review the diff and restore only artifacts created by that verification; preserve unrelated user changes.

## Watching GitHub Actions

- Confirm the replacement workflow is attached to the expected head SHA.
- Use `gh run watch <run-id> --repo qarax/qarax --exit-status` until the workflow terminates.
- If one job fails while another is still running, job logs may be available through `gh api --allow-escape-sequences repos/qarax/qarax/actions/jobs/<job-id>/logs` before `gh run view --log-failed` permits access.
- Inspect every failed job and the first actionable error in each. A successful build does not make the PR green if the combined checks failed.
- E2E being skipped is expected on an unlabeled PR; state that explicitly rather than calling it a pass.

## Repo-specific reminders

- `make lint` runs `cargo clippy --workspace -- -D warnings`.
- CI Clippy enables `qarax/otel,qarax-node/otel`; `make lint` alone is not feature-equivalent.
- `make test` may auto-start PostgreSQL via Docker unless `SKIP_DOCKER=1` is set.
- CI uses nightly for format checks.
- Do not edit generated files such as `openapi.yaml` or `python-sdk/qarax-api-client/` directly; regenerate them from the source.
- Read service and workflow logs before making code changes.

## Avoid

- guessing from the final line of the log only
- treating cascade errors as independent failures
- editing code before checking whether the environment is broken
- pushing to a remote without explicit authorization
- claiming a pushed fix succeeded before its replacement run completes
- adding broad fallbacks that only make CI pass superficially

## Validation

- Re-run the specific failing command locally first.
- Then run the broader repo validation needed for the touched area.
- Do not declare success until the failure is reproduced or credibly explained and the fix is validated.
