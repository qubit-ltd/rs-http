# HTTP-R2 Compatibility Implementation Evidence

Date: 2026-11-06

> Scope update (2026-11-07): the repository CI and test suite no longer run or maintain the downstream compatibility runner. The recorded run below is historical evidence from the implementation commit, not a current CI gate. The downstream core/provider source migration remains in those repositories.

## Scope

- HTTP-R2-001: add a repeatable source-snapshot compatibility check with a pinned SDK baseline, machine-readable result, command logs, and CI unit-test entry.
- HTTP-R2-002: update the SDK core/provider requirements to `qubit-http ^0.16`, reuse `HttpResponse::validate_sse_content_type()`, and preserve OpenAI error mapping and stream policies.

## Fixed inputs and result

The compatibility runner completed in fixed-baseline mode. Its temporary source root was `/tmp/rs-http-compat-source-8614a8ab`; the root project tree was `/data/working/qubit`. The input repositories were clean detached Git worktrees at the commits recorded in `tests/downstream_compatibility_baseline.json`. This kept existing local changes in `rs-config`, `rs-json`, and `rs-value` out of the validation input without changing those checkouts.

Command:

```sh
python3 /tmp/rs-http-http-r2-implementation/tests/check_downstream_compatibility.py --source-root /tmp/rs-http-compat-source-8614a8ab --output-dir /tmp/rs-http-compat-8614a8ab-final
```

Result: exit 0, `status=passed`. Both `cargo metadata --format-version 1` commands exited 0 and each dependency graph resolved exactly one `qubit-http 0.16.0` package at the isolated `source/rust-common/rs-http/Cargo.toml`.

| Repository | Filter | Exit | Tests passed |
| --- | --- | ---: | ---: |
| llmsdk-core | `remote_engine_parts` | 0 | 20 |
| llmsdk-provider-openai | `test_call_retry_classifies_rate_limit` | 0 | 1 |
| llmsdk-provider-openai | `test_quota_and_unclassified_429_abort_without_retry` | 0 | 1 |
| llmsdk-provider-openai | `openai_chat_engine_tests` | 0 | 31 |
| llmsdk-provider-openai | `openai_responses_engine_tests` | 0 | 26 |
| llmsdk-provider-openai | `sse_content_type_contract` | 0 | 2 |

The runner recorded Cargo 1.94.0 and Rust 1.94.0. Source `Cargo.toml`/`Cargo.lock` digests were identical before and after execution. The tested `rs-http` input was identified as a working-tree snapshot (SHA-256 `5a3b6339e3b774549acec861c31de1f697ee5fd411090c51005a53d4295f7eb3`). Snapshot lockfile digests, including the generated core/provider resolutions, were captured in the temporary `result.json`; that file is removed during final temporary-file cleanup.

## Other validation

- `python3 -B -m unittest discover -s tests -p check_downstream_compatibility_tests.py`: exit 0, 21 tests passed.
- `python3 tests/check_guide_examples.py && python3 -B -m unittest discover -s tests -p check_downstream_compatibility_tests.py`: exit 0.
- Target `align-ci.sh`: exit 0.
- Target `ci-check.sh`: final rerun exit 0. The existing thresholds were retained; coverage was functions 97.16%, lines 96.86%, and regions 96.07%. Dependency audit passed. An earlier full run had one intermittent failure in `test_downstream_contract_outer_two_attempts_disable_internal_http_retries` (expected HTTP 429, observed no status); the focused test and full `tests/mod.rs` rerun passed, and the complete `ci-check.sh` rerun passed.
- `git diff --check`: exit 0.

The SDK core/provider `align-ci.sh` and `ci-check.sh` commands both exit 1 before compiling because their repositories contain pre-existing Rust style/Rustdoc issues (158 in core and 113 in provider). The T2 diff does not touch the reported core files; provider reports include unrelated existing Rustdoc issues in untouched items. These broad baseline issues were left unchanged to keep this task scoped.

## Registry limitation

`cargo info qubit-http@0.16.0` exits 101 because crates.io currently has no 0.16.0 release. `cargo metadata --locked` in core and provider likewise cannot resolve `^0.16`; applying the isolated local source patch with `--locked --offline` exits 101 because Cargo must update the lockfile. Therefore the source snapshot compatibility check passed, but formal registry consumption and authentic registry lockfile updates remain unverified. No release or publication was performed.

The temporary runner output and source worktrees are execution artifacts; this report preserves their relevant summaries. The full logs and `result.json` are not retained after temporary-file cleanup.
