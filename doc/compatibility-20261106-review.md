# Downstream Compatibility Check (2026-11-06)

## Scope and baselines

This is an isolated source-snapshot check for HTTP-002. It is not an SDK dependency upgrade or a claim of full downstream compatibility.

| Repository | Original branch | Original HEAD | Original status | Declared `qubit-http` |
| --- | --- | --- | --- | --- |
| `rs-llmsdk-core` | `dev-starfish` | `b31c44d4cded0c260a02c2aac59c45cce65570b7` | clean (`dev-starfish...origin/dev-starfish`) | `^0.15` |
| `rs-llmsdk-provider-openai` | `main` | `1231c34dc5989910ddcffdff9d7a738381b03323` | clean (`main...origin/main`) | `^0.15` |

The original SDK worktrees were read-only during this check. Snapshots were made under `/tmp/rs-http-review-sdk-snapshot-20261106`, excluding `.git` and `target`. In the snapshot manifests only, the `qubit-http` requirement was changed to version `0.16` and a path pointing to `/data/working/qubit/rust-common/rs-http-review-20261106`. Existing relative paths between core, provider, and agent were retained. Test build artifacts were placed under `/tmp/rs-http-review-sdk-target`.

## Resolution evidence

- `cargo metadata --format-version 1 --manifest-path /tmp/rs-http-review-sdk-snapshot-20261106/rs-llmsdk-core/Cargo.toml` — exit 0.
- `cargo tree --manifest-path /tmp/rs-http-review-sdk-snapshot-20261106/rs-llmsdk-core/Cargo.toml -i qubit-http` — exit 0; resolved only `qubit-http v0.16.0 (/data/working/qubit/rust-common/rs-http-review-20261106)` through core.
- `cargo metadata --format-version 1 --manifest-path /tmp/rs-http-review-sdk-snapshot-20261106/rs-llmsdk-provider-openai/Cargo.toml` — exit 0.
- `cargo tree --manifest-path /tmp/rs-http-review-sdk-snapshot-20261106/rs-llmsdk-provider-openai/Cargo.toml -i qubit-http` — exit 0; resolved only the same path package through provider, core, and agent. No registry 0.15 package or duplicate `qubit-http` type was present.

The resolver also selected local path packages required by `qubit-http` (`qubit-config`, `qubit-json`, `qubit-redact`, and `qubit-value`) from their existing sibling repositories. No source SDK manifest or lockfile was changed.

## SDK test results

All commands used the snapshot manifests and the shared temporary target directory.

| Command | Result |
| --- | --- |
| `CARGO_TARGET_DIR=/tmp/rs-http-review-sdk-target cargo test --manifest-path /tmp/rs-http-review-sdk-snapshot-20261106/rs-llmsdk-core/Cargo.toml --test mod remote_engine_parts` | exit 0; 20 passed, 0 failed |
| `CARGO_TARGET_DIR=/tmp/rs-http-review-sdk-target cargo test --manifest-path /tmp/rs-http-review-sdk-snapshot-20261106/rs-llmsdk-provider-openai/Cargo.toml --test mod test_call_retry_classifies_rate_limit` | exit 0; 1 passed, 0 failed |
| `CARGO_TARGET_DIR=/tmp/rs-http-review-sdk-target cargo test --manifest-path /tmp/rs-http-review-sdk-snapshot-20261106/rs-llmsdk-provider-openai/Cargo.toml --test mod test_quota_and_unclassified_429_abort_without_retry` | exit 0; 1 passed, 0 failed |
| `CARGO_TARGET_DIR=/tmp/rs-http-review-sdk-target cargo test --manifest-path /tmp/rs-http-review-sdk-snapshot-20261106/rs-llmsdk-provider-openai/Cargo.toml --test mod openai_chat_engine_tests` | exit 0; 30 passed, 0 failed |
| `CARGO_TARGET_DIR=/tmp/rs-http-review-sdk-target cargo test --manifest-path /tmp/rs-http-review-sdk-snapshot-20261106/rs-llmsdk-provider-openai/Cargo.toml --test mod openai_responses_engine_tests` | exit 0; 25 passed, 0 failed |

The SDK builds emit the existing deprecation warning for `HttpClientBuilder::create` in core. It does not fail the tested targets and was not changed in the isolated snapshots.

## Findings and migration direction

The suspected 0.16 incompatibility was **not reproduced** in these targeted builds and tests. No `rs-http` defect or required source compatibility fix was found. This targeted evidence does not replace broader SDK CI or a release upgrade.

The provider currently duplicates the SSE media-type check in `src/chat/openai_chat_client.rs::execute_stream_call` and `src/chat/openai_responses_client.rs::execute_stream_call`. If the provider later adopts `validate_sse_content_type()`, it can replace each local `Content-Type` parse/check before consuming the response. Preserve the provider-specific `EngineError` mapping and Responses `error_context` when mapping an `HttpError`; the generic HTTP library must not take over OpenAI error classification. The SDK repositories were not modified as part of this check.
