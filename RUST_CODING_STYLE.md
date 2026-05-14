# Rust Coding Style

[中文版本](RUST_CODING_STYLE.zh_CN.md)

This guide documents the Rust style expected for contributions to `qubit-http`.
It complements `rustfmt`, `clippy`, and the existing codebase conventions.

## Guiding Principles

- Follow the existing module layout, naming, error model, and documentation style.
- Prefer clear, readable, verifiable code over clever abstractions.
- Keep changes focused. Avoid broad refactors unless they are required by the change.
- Public behavior changes should include tests and user-facing documentation updates.
- Do not expose the full `reqwest` API just for convenience; this crate provides a stable HTTP infrastructure layer.

## Formatting And Tooling

Before opening a pull request, run the relevant checks locally:

```bash
./style-check.sh
./ci-check.sh
```

Use `./align-ci.sh` to format code before rerunning the checks. Do not use a
bare `cargo fmt` as the canonical formatter command for this project; the CI
formatting rules use the project rustfmt configuration through `align-ci.sh`.
Use `./style-check.sh` for the style-only check and `./ci-check.sh` for the
full local CI check before committing larger changes.

If a check cannot be run in your environment, mention that in the pull request.

## Module Organization

- The `src/` directory is for production source files only. Do not put test code
  in `src/`, and do not add `#[cfg(test)]` test modules to source files.
- All test code belongs under `tests/`. Shared test fixtures and integration
  helpers may live under `tests/common` or another clearly named test-only
  module.
- Source files use `snake_case.rs`.
- As a rule, each public `struct`, `trait`, or type alias should live in its own
  source file. The file name must be the public type name converted to
  lower_snake_case, for example `HttpClientOptions` in
  `http_client_options.rs`.
- Closely related internal implementation types may live in the same source
  file when separating them would make the code harder to understand.
- Each source file should have a corresponding unit test file under `tests/`
  at the same relative module path. The test file name is the source file stem
  plus `_tests.rs`; for example `src/options/proxy_options.rs` maps to
  `tests/options/proxy_options_tests.rs`.
- Use `mod.rs` exports consistently with the existing test and source layout.
- Keep visibility narrow: prefer private items, then `pub(crate)`, then public API only when needed.
- Add new public exports only when they are part of the intended crate API.

## API Design

- Prefer `&str` for string inputs unless ownership is required.
- Prefer slices (`&[T]`) for collection inputs instead of `&Vec<T>`.
- Avoid unnecessary `String`, `Vec`, `clone`, and allocation in hot paths.
- Use builder-style APIs when a type has multiple optional settings.
- Represent state with enums when the alternatives are meaningful; avoid clusters of booleans for state machines.
- Keep async APIs explicit: return `Result<T, HttpError>` or another concrete project error type.
- Avoid blocking I/O in async code.
- Keep shared-state critical sections short and easy to reason about.

## Error Handling

- Public APIs should return concrete error types. In this crate, prefer `HttpError`, `HttpConfigError`, or `HttpResult<T>`.
- Preserve context in error messages: method, URL, status, timeout phase, config key, or retry state when relevant.
- Avoid `unwrap()` and `expect()` in production code. If an invariant requires `expect()`, make the message specific.
- Preserve retry semantics by using `HttpErrorKind` and `RetryHint` consistently.
- Map backend errors through the existing error-mapping code instead of introducing ad hoc conversions.

## Documentation Comments

The project uses English Rust documentation comments.

- Use `//!` for module-level documentation.
- Public types and non-trivial public methods should explain purpose, key constraints, and correct usage.
- Function and method docs should cover parameters, return values, errors, and important side effects.
- `Result` docs should explain when `Err` is returned and what context the error carries.
- `Option` docs should explain what `Some` and `None` mean when the meaning is not obvious.
- Document async I/O, cancellation, retries, logging side effects, and stream consumption behavior.
- Keep examples focused and compatible with doctest expectations where possible.

## Tests

- Put tests under `tests/`.
- Name test files with a `_tests.rs` suffix.
- Do not declare a top-level `tests/*.rs` integration test target again from
  `tests/mod.rs`; Cargo already runs it as a separate target.
- Name tests with the `test_{method_or_behavior}_{scenario}` pattern.
- Cover normal paths, error paths, boundary conditions, and regressions.
- Prefer precise assertions over broad “it failed” checks.
- Avoid external network dependencies in tests.
- Use `expect()` with useful messages instead of casual `unwrap()` in tests.

## Logging And Security

- Respect sensitive header masking behavior.
- Do not log secrets, full credentials, or raw binary bodies.
- Keep body previews bounded.
- When changing logging behavior, test header masking and body truncation cases.

## Performance

- Avoid unnecessary body buffering; response bodies should remain lazy unless behavior explicitly requires buffering.
- Preallocate collections when the size is known.
- Avoid cloning request or response bodies unless needed for retry, logging, or ownership boundaries.
- Consider stream backpressure and cancellation when adding streaming behavior.

## Documentation Updates

Update the relevant documentation when public behavior changes:

- `README.md` and `README.zh_CN.md` for project-level positioning and entry points.
- `doc/user_guide.en.md` and `doc/user_guide.zh_CN.md` for usage details.
- Rustdoc comments for API contracts and edge cases.
