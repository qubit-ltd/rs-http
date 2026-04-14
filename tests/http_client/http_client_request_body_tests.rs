/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use std::time::Duration;

use bytes::Bytes;
use http::Method;
use qubit_http::{HttpClientFactory, HttpClientOptions};
use tokio::time::timeout;

use crate::common::{spawn_one_shot_server, ResponsePlan};

#[tokio::test]
async fn test_execute_with_form_body_and_query_headers_timeout() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientFactory::new()
        .create_with_options(options)
        .expect("client should be created");

    let request = client
        .request(Method::POST, "/form")
        .query_param("kind", "form")
        .header("x-test", "1")
        .expect("header should be valid")
        .form_body([("name", "alice"), ("city", "shanghai")])
        .timeout(Duration::from_secs(1))
        .build();
    timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("form request should succeed");

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/form?kind=form");
    assert_eq!(
        captured.headers.get("content-type"),
        Some(&"application/x-www-form-urlencoded".to_string())
    );
    let body = String::from_utf8(captured.body).expect("body should be utf-8");
    assert!(body.contains("name=alice"));
    assert!(body.contains("city=shanghai"));
}

#[tokio::test]
async fn test_execute_with_multipart_body_and_query_headers_timeout() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientFactory::new()
        .create_with_options(options)
        .expect("client should be created");

    let payload = Bytes::from_static(
        b"--abc\r\nContent-Disposition: form-data; name=\"field\"\r\n\r\nvalue\r\n--abc--\r\n",
    );
    let request = client
        .request(Method::POST, "/multipart")
        .query_param("kind", "multipart")
        .header("x-test", "1")
        .expect("header should be valid")
        .multipart_body(payload.clone(), "abc")
        .expect("multipart body should be built")
        .timeout(Duration::from_secs(1))
        .build();
    timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("multipart request should succeed");

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/multipart?kind=multipart");
    assert_eq!(
        captured.headers.get("content-type"),
        Some(&"multipart/form-data; boundary=abc".to_string())
    );
    assert_eq!(captured.body, payload.to_vec());
}

#[tokio::test]
async fn test_execute_with_ndjson_body_and_query_headers_timeout() {
    #[derive(serde::Serialize)]
    struct Record {
        id: i32,
        name: &'static str,
    }

    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientFactory::new()
        .create_with_options(options)
        .expect("client should be created");

    let request = client
        .request(Method::POST, "/ndjson")
        .query_param("kind", "ndjson")
        .header("x-test", "1")
        .expect("header should be valid")
        .ndjson_body(&[
            Record {
                id: 1,
                name: "alpha",
            },
            Record {
                id: 2,
                name: "beta",
            },
        ])
        .expect("ndjson should be encoded")
        .timeout(Duration::from_secs(1))
        .build();
    timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("ndjson request should succeed");

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/ndjson?kind=ndjson");
    assert_eq!(
        captured.headers.get("content-type"),
        Some(&"application/x-ndjson".to_string())
    );
    let body = String::from_utf8(captured.body).expect("body should be utf-8");
    assert_eq!(
        body,
        "{\"id\":1,\"name\":\"alpha\"}\n{\"id\":2,\"name\":\"beta\"}\n"
    );
}

#[tokio::test]
async fn test_execute_with_stream_body_uses_chunked_transfer_encoding() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientFactory::new()
        .create_with_options(options)
        .expect("client should be created");

    let request = client
        .request(Method::POST, "/stream-upload")
        .query_param("kind", "stream")
        .stream_body([
            Bytes::from_static(b"first-"),
            Bytes::from_static(b"second-"),
            Bytes::from_static(b"third"),
        ])
        .timeout(Duration::from_secs(1))
        .build();
    let response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("stream body request should succeed");
    assert_eq!(response.meta.status.as_u16(), 200);

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/stream-upload?kind=stream");
    assert_eq!(
        captured.headers.get("transfer-encoding"),
        Some(&"chunked".to_string())
    );
}

#[tokio::test]
async fn test_execute_stream_with_stream_body_uses_chunked_transfer_encoding() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientFactory::new()
        .create_with_options(options)
        .expect("client should be created");

    let request = client
        .request(Method::POST, "/stream-upload-streaming")
        .query_param("kind", "stream-body")
        .stream_body([Bytes::from_static(b"a"), Bytes::from_static(b"b")])
        .timeout(Duration::from_secs(1))
        .build();
    let response = timeout(Duration::from_secs(3), client.execute_stream(request))
        .await
        .expect("execute_stream timed out")
        .expect("stream body request should succeed");
    assert_eq!(response.status().as_u16(), 200);

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/stream-upload-streaming?kind=stream-body");
    assert_eq!(
        captured.headers.get("transfer-encoding"),
        Some(&"chunked".to_string())
    );
}
