/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

//! Coverage-only tests for defensive branches.

use qubit_http::{coverage_support, HttpErrorKind};

use crate::common::{spawn_one_shot_server, ResponsePlan};

#[tokio::test]
async fn test_coverage_support_exercises_backend_error_mapper_paths() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 500,
        headers: vec![],
        body: Vec::new(),
    })
    .await;
    let status_error = reqwest::Client::new()
        .get(server.base_url())
        .send()
        .await
        .expect("coverage status response should be received")
        .error_for_status()
        .expect_err("coverage status response should become reqwest status error");

    assert_eq!(
        coverage_support::map_reqwest_error_kind(status_error, None, HttpErrorKind::Transport,),
        HttpErrorKind::Status
    );
    assert_eq!(
        coverage_support::classify_backend_error_kinds(),
        vec![
            HttpErrorKind::Decode,
            HttpErrorKind::Status,
            HttpErrorKind::InvalidUrl,
            HttpErrorKind::Transport,
        ]
    );

    assert_eq!(
        coverage_support::classify_timeout_kinds(),
        vec![
            HttpErrorKind::ConnectTimeout,
            HttpErrorKind::RequestTimeout,
            HttpErrorKind::ReadTimeout,
            HttpErrorKind::RequestTimeout,
        ]
    );
}

#[test]
fn test_coverage_support_exercises_http_retry_mapping_paths() {
    let diagnostics = coverage_support::exercise_http_retry_mapping_paths();

    assert!(diagnostics
        .iter()
        .any(|message| message.contains("attempts exhausted")),);
    assert!(diagnostics
        .iter()
        .any(|message| message.contains("retry aborted")),);
    assert!(diagnostics
        .iter()
        .any(|message| message.contains("retry executor failed")),);
    assert!(diagnostics
        .iter()
        .any(|message| message.contains("max duration exceeded")),);
    assert_eq!(
        coverage_support::exercise_retry_failure_decision_path(),
        "UseDefault"
    );
}

#[tokio::test]
async fn test_coverage_support_exercises_response_and_request_paths() {
    let (kind, message) = coverage_support::validate_non_utf8_sse_content_type();
    assert_eq!(kind, HttpErrorKind::SseProtocol);
    assert!(message.contains("non-UTF8 Content-Type"));

    let response_diagnostics = coverage_support::exercise_response_preview_paths().await;
    assert!(response_diagnostics
        .iter()
        .any(|message| message.contains("abc")),);
    assert!(response_diagnostics
        .iter()
        .any(|message| message.contains("<empty>")),);
    assert!(response_diagnostics
        .iter()
        .any(|message| message.contains("Cancelled")),);

    let request_diagnostics = coverage_support::exercise_request_cache_paths().await;
    assert!(request_diagnostics
        .iter()
        .any(|message| message.contains("coverage-cache")),);
    assert!(request_diagnostics.iter().any(|message| message == "0"),);
}

#[tokio::test]
async fn test_coverage_support_exercises_threshold_paths() {
    let diagnostics = coverage_support::exercise_threshold_paths().await;

    assert!(diagnostics
        .iter()
        .any(|message| message.contains("No IPv4 address found")),);
    assert!(diagnostics.iter().any(|message| message == "1"),);
    assert!(diagnostics.iter().any(|message| message == "coverage"),);
    assert!(diagnostics.iter().any(|message| message == "ProxyConfig"),);
    assert!(diagnostics.iter().any(|message| message == "Other"),);
    assert!(diagnostics
        .iter()
        .any(|message| message == "/relative-only"),);
    assert!(diagnostics.iter().any(|message| message == "Cancelled"),);
}
