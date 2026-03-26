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
4. Reproduce locally using existing repo commands:
   - `make fmt`
   - `make lint`
   - `make build`
   - `make test`
   - narrower `cargo build -p ...` or `cargo nextest run -p ...` commands when appropriate
5. For SQL query changes, run `cargo sqlx prepare --workspace`.
6. For user-facing changes, verify CLI and E2E impact as needed.
7. Prefer fixing all required fields, arguments, or call sites together after reviewing full signatures and interfaces.
8. Validate with the same or stricter checks than CI before considering the issue fixed.

## Repo-specific reminders

- `make lint` runs `cargo clippy --workspace -- -D warnings`.
- `make test` may auto-start PostgreSQL via Docker unless `SKIP_DOCKER=1` is set.
- CI uses nightly for format checks.
- Do not edit generated files such as `openapi.yaml` or `python-sdk/` directly; regenerate them from the source.
- Read service and workflow logs before making code changes.

## Avoid

- guessing from the final line of the log only
- treating cascade errors as independent failures
- editing code before checking whether the environment is broken
- pushing to a remote as part of the debugging procedure
- adding broad fallbacks that only make CI pass superficially

## Validation

- Re-run the specific failing command locally first.
- Then run the broader repo validation needed for the touched area.
- Do not declare success until the failure is reproduced or credibly explained and the fix is validated.
