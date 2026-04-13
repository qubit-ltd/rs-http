/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use std::time::Duration;

use http::{HeaderMap, HeaderName, HeaderValue, Method};
use qubit_http::{HeaderInjector, HttpClientFactory, HttpClientOptions, HttpError, HttpErrorKind};
use tokio::time::timeout;

use crate::common::{spawn_one_shot_server, ResponsePlan};

#[test]
fn test_http_client_debug_includes_options_and_injectors() {
    let mut client = HttpClientFactory::new()
        .create()
        .expect("default options should create client");
    client.add_header_injector(HeaderInjector::new(|_headers: &mut HeaderMap| Ok(())));

    let output = format!("{:?}", client);

    assert!(output.contains("HttpClient"));
    assert!(output.contains("options"));
    assert!(output.contains("injectors"));
}

#[tokio::test]
async fn test_absolute_url_request_bypasses_base_url_join() {
    let target_server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    // Deliberately points to a non-existing host; absolute URL should bypass this.
    options.base_url = Some(url::Url::parse("http://127.0.0.1:1/").unwrap());
    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();

    let path = format!("{}absolute", target_server.base_url());
    let request = client.request(Method::GET, path.as_str()).build();
    let response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    assert_eq!(response.status.as_u16(), 200);

    let captured = timeout(Duration::from_secs(3), target_server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/absolute");
}

#[tokio::test]
async fn test_execute_returns_invalid_url_for_bad_relative_path() {
    let mut options = HttpClientOptions::default();
    options.base_url = Some(
        url::Url::parse("https://example.com/api/").expect("static base_url in test should parse"),
    );
    let client = HttpClientFactory::new()
        .create_with_options(options)
        .expect("valid options should create client");
    let request = client.request(Method::GET, "http://[::1").build();

    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect_err("invalid relative path should fail before request is sent");

    assert_eq!(error.kind, HttpErrorKind::InvalidUrl);
    assert!(error
        .message
        .contains("Failed to resolve path 'http://[::1'"));
}

#[tokio::test]
async fn test_header_injector_order_is_stable_and_clear_works() {
    let server1 = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server1.base_url());
    let mut client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();
    client.add_header_injector(HeaderInjector::new(|headers: &mut HeaderMap| {
        headers.insert(
            HeaderName::from_static("x-seq"),
            HeaderValue::from_static("A"),
        );
        Ok(())
    }));
    client.add_header_injector(HeaderInjector::new(|headers: &mut HeaderMap| {
        headers.insert(
            HeaderName::from_static("x-seq"),
            HeaderValue::from_static("B"),
        );
        Ok(())
    }));

    let request = client.request(Method::GET, "/ordered").build();
    let _ = client.execute(request).await.unwrap();
    let captured = server1.finish().await;
    assert_eq!(captured.headers.get("x-seq"), Some(&"B".to_string()));

    let server2 = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options2 = HttpClientOptions::default();
    options2.base_url = Some(server2.base_url());
    let mut client2 = HttpClientFactory::new()
        .create_with_options(options2)
        .unwrap();
    client2.add_header_injector(HeaderInjector::new(|headers: &mut HeaderMap| {
        headers.insert(
            HeaderName::from_static("x-seq"),
            HeaderValue::from_static("A"),
        );
        Ok(())
    }));
    client2.clear_header_injectors();
    let request2 = client2.request(Method::GET, "/cleared").build();
    let _ = client2.execute(request2).await.unwrap();
    let captured2 = server2.finish().await;
    assert!(!captured2.headers.contains_key("x-seq"));
}

#[tokio::test]
async fn test_failing_header_injector_short_circuits_request() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();
    client.add_header_injector(HeaderInjector::new(|_headers: &mut HeaderMap| {
        Err(HttpError::other("inject failed"))
    }));

    let request = client.request(Method::GET, "/will-not-send").build();
    let error = client.execute(request).await.unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("inject failed"));
}

#[tokio::test]
async fn test_add_header_applies_client_default_header() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();
    client.add_header("x-client", "default").unwrap();

    let request = client.request(Method::GET, "/default-header").build();
    let _ = client.execute(request).await.unwrap();
    let captured = server.finish().await;
    assert_eq!(
        captured.headers.get("x-client"),
        Some(&"default".to_string())
    );
}

#[tokio::test]
async fn test_add_headers_is_atomic_and_request_header_still_overrides() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();
    client
        .add_headers([
            ("x-batch-a", "value-a"),
            ("x-order", "client"),
            ("x-batch-b", "value-b"),
        ])
        .unwrap();

    let request = client
        .request(Method::GET, "/batch-headers")
        .header("x-order", "request")
        .unwrap()
        .build();
    let _ = client.execute(request).await.unwrap();
    let captured = server.finish().await;
    assert_eq!(
        captured.headers.get("x-batch-a"),
        Some(&"value-a".to_string())
    );
    assert_eq!(
        captured.headers.get("x-batch-b"),
        Some(&"value-b".to_string())
    );
    assert_eq!(
        captured.headers.get("x-order"),
        Some(&"request".to_string())
    );
}

#[tokio::test]
async fn test_add_headers_invalid_batch_does_not_partially_apply() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();

    let error = client
        .add_headers([("x-valid", "kept-out"), ("bad header", "boom")])
        .unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("Invalid header name"));

    let request = client.request(Method::GET, "/atomic").build();
    let _ = client.execute(request).await.unwrap();
    let captured = server.finish().await;
    assert!(!captured.headers.contains_key("x-valid"));
}

#[tokio::test]
async fn test_add_header_invalid_value_does_not_apply() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();

    let error = client.add_header("x-bad", "line1\nline2").unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("Invalid header value"));

    let request = client.request(Method::GET, "/invalid-value").build();
    let _ = client.execute(request).await.unwrap();
    let captured = server.finish().await;
    assert!(!captured.headers.contains_key("x-bad"));
}

#[tokio::test]
async fn test_add_header_injector_still_overrides_client_default_header() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();
    client.add_header("x-order", "client").unwrap();
    client.add_header_injector(HeaderInjector::new(|headers: &mut HeaderMap| {
        headers.insert(
            HeaderName::from_static("x-order"),
            HeaderValue::from_static("injector"),
        );
        Ok(())
    }));

    let request = client.request(Method::GET, "/injector-overrides").build();
    let _ = client.execute(request).await.unwrap();
    let captured = server.finish().await;
    assert_eq!(
        captured.headers.get("x-order"),
        Some(&"injector".to_string())
    );
}

#[tokio::test]
async fn test_clone_default_headers_are_independent_after_creation() {
    let server_original = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let server_clone = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;

    let mut client = HttpClientFactory::new().create().unwrap();
    client.add_header("x-shared", "base").unwrap();

    let mut cloned = client.clone();
    cloned.add_header("x-clone-only", "yes").unwrap();
    client.add_header("x-origin-only", "yes").unwrap();

    let path = format!("{}origin", server_original.base_url());
    let request_original = client.request(Method::GET, path.as_str()).build();
    let path = format!("{}clone", server_clone.base_url());
    let request_clone = cloned.request(Method::GET, path.as_str()).build();

    let _ = client.execute(request_original).await.unwrap();
    let _ = cloned.execute(request_clone).await.unwrap();

    let captured_original = server_original.finish().await;
    let captured_clone = server_clone.finish().await;

    assert_eq!(
        captured_original.headers.get("x-shared"),
        Some(&"base".to_string())
    );
    assert_eq!(
        captured_clone.headers.get("x-shared"),
        Some(&"base".to_string())
    );
    assert_eq!(
        captured_original.headers.get("x-origin-only"),
        Some(&"yes".to_string())
    );
    assert!(!captured_original.headers.contains_key("x-clone-only"));
    assert_eq!(
        captured_clone.headers.get("x-clone-only"),
        Some(&"yes".to_string())
    );
    assert!(!captured_clone.headers.contains_key("x-origin-only"));
}
