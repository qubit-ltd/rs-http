/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::result_large_err)]
// Integration tests favor readability of scenario setup; partial Default mutation and
// direct `HttpError` assertions are intentional.

//! # qubit-http integration tests
//!
//! Submodules mirror `src/` layout; paths are explicit so nested files are not
//! registered as separate integration test crates (same pattern as `qubit-config`).

#[path = "common/mod.rs"]
mod common;

#[path = "options/http_client_options_tests.rs"]
mod http_client_options_tests;
#[path = "options/http_config_error_kind_tests.rs"]
mod http_config_error_kind_tests;
#[path = "options/http_config_error_tests.rs"]
mod http_config_error_tests;
#[path = "options/http_retry_options_tests.rs"]
mod http_retry_options_tests;
#[path = "options/logging_options_tests.rs"]
mod logging_options_tests;
#[path = "options/proxy_options_tests.rs"]
mod proxy_options_tests;
#[path = "options/proxy_type_tests.rs"]
mod proxy_type_tests;
#[path = "options/sensitive_headers_tests.rs"]
mod sensitive_headers_tests;
#[path = "options/timeout_options_tests.rs"]
mod timeout_options_tests;

#[path = "factory/ipv4_only_tests.rs"]
mod ipv4_only_tests;
#[path = "factory/reqwest_http_client_factory_tests.rs"]
mod reqwest_http_client_factory_tests;

#[path = "error/http_error_tests.rs"]
mod http_error_tests;

#[path = "logging/logging_policy_tests.rs"]
mod logging_policy_tests;
#[path = "logging/masker_tests.rs"]
mod masker_tests;

#[path = "request/http_request_builder_tests.rs"]
mod http_request_builder_tests;

#[path = "sse/done_marker_policy_tests.rs"]
mod done_marker_policy_tests;
#[path = "sse/json_decoder_tests.rs"]
mod json_decoder_tests;
#[path = "sse/sse_event_tests.rs"]
mod sse_event_tests;
#[path = "sse/mod_tests.rs"]
mod sse_mod_tests;

#[path = "http_client/http_client_behavior_tests.rs"]
mod http_client_behavior_tests;
#[path = "http_client/http_client_cancel_tests.rs"]
mod http_client_cancel_tests;
#[path = "http_client/http_client_tests.rs"]
mod http_client_tests;
#[path = "http_client/http_client_timeout_tests.rs"]
mod http_client_timeout_tests;
#[path = "http_client/http_response_tests.rs"]
mod http_response_tests;
#[path = "http_client/http_stream_response_tests.rs"]
mod http_stream_response_tests;

#[path = "proxy/proxy_tests.rs"]
mod proxy_tests;
#[path = "proxy/socks5_proxy_tests.rs"]
mod socks5_proxy_tests;

#[path = "retry_hint/retry_hint_tests.rs"]
mod retry_hint_tests;

#[path = "sse/frame_decoder_tests.rs"]
mod frame_decoder_tests;
#[path = "sse/line_decoder_tests.rs"]
mod line_decoder_tests;
#[path = "sse/sse_integration_tests.rs"]
mod sse_integration_tests;
