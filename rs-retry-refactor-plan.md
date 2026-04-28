# rs-retry Refactor Tracking Plan

This file tracks the rs-retry-first refactor requested from the rs-http review
thread. Keep it updated after each completed step.

## Scope

- First refactor `rust-common/rs-retry`.
- Bump `qubit-retry` with a breaking-change minor version.
- Add and update tests, README, and docs for the retry elapsed semantics.
- Run `align-ci.sh` and `ci-check.sh` in `rs-retry`.
- Commit only `rs-retry` changes with English commit messages.
- Stop and notify the user to publish `qubit-retry`.
- Refactor `rs-http` only after the user confirms the new `qubit-retry` release is published.

## Current Decisions

- Rename old operation-only `max_elapsed` to `max_operation_elapsed`.
- Add `max_total_elapsed` for full retry-flow monotonic elapsed time.
- `max_operation_elapsed` includes operation execution time only.
- `max_total_elapsed` includes operation execution, retry sleep / Retry-After sleep, retry hint extraction, `on_before_attempt`, `on_failure`, and `on_retry` time.
- Terminal listeners keep notification semantics and must not turn an already successful operation into a retry failure.
- Retry sleep is not truncated. If the remaining total budget cannot cover the next delay, fail before sleeping.
- All elapsed budgets are measured with monotonic time via `Instant`, not wall-clock time.
- `rs-http` should later map retry `max_duration` to `max_total_elapsed` and reuse `rs-retry`.

## Checklist

- [x] Read project rules and verify no extra `AGENTS.md` applies.
- [x] Inspect current `rs-retry` retry, options, config, tests, and docs.
- [x] Decide elapsed-budget semantics with the user.
- [x] Rename operation-only API and internals from `max_elapsed` to `max_operation_elapsed`.
- [x] Add `max_total_elapsed` to options, config, context, and retry execution.
- [x] Ensure total elapsed includes operation time, retry sleep, Retry-After sleep, and retry control-path listener time.
- [x] Ensure budget-short sleep fails before sleeping.
- [x] Update README, Chinese README, and design docs.
- [x] Bump `qubit-retry` version from `0.8.1` to `0.9.0`.
- [x] Add/update unit and async/worker retry tests.
- [x] Run `cargo fmt`.
- [x] Run `./align-ci.sh`.
- [x] Run `./ci-check.sh`.
- [x] Review `git diff` for `rs-retry` only.
- [x] Commit `rs-retry` changes with English commit messages.
- [x] Notify the user to publish `qubit-retry`.
- [x] Confirm `qubit-retry` 0.9.0 is published.
- [x] Upgrade `rs-http` dependency to `qubit-retry` 0.9.x.
- [x] Map HTTP retry `max_duration` to `rs-retry` `max_total_elapsed`.
- [x] Remove obsolete public retry helper and `PendingHttpRetryAfterDelay` export.
- [x] Update SSE reconnect retry options to the new elapsed-budget API.
- [x] Fix strict clippy warning in SSE reconnect elapsed check.
- [x] Add/update rs-http retry, cancellation, timeout, and public API tests.
- [x] Update rs-http user-facing docs for retry semantics.
- [x] Add coverage-targeted rs-http tests and remove low-value uncovered branches so per-source thresholds pass.
- [x] Run `cargo fmt`.
- [x] Run `./align-ci.sh`.
- [x] Run `./ci-check.sh`. Current status: all seven local CI stages pass, including coverage thresholds and security audit.
- [x] Review `git diff` for `rs-http`.
- [x] Commit `rs-http` changes with English commit messages.
