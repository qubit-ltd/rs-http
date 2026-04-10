/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use std::sync::Arc;
use std::time::Duration;

use http::{HeaderMap, HeaderName, HeaderValue, Method};
use qubit_http::{HeaderInjector, HttpClientFactory, HttpClientOptions, HttpError, HttpErrorKind, HttpResult};
use tokio::time::timeout;

use crate::common::{spawn_one_shot_server, ResponsePlan};

#[derive(Debug)]
struct InjectorA;

impl HeaderInjector for InjectorA {
    fn inject(&self, headers: &mut HeaderMap) -> HttpResult<()> {
        headers.insert(
            HeaderName::from_static("x-seq"),
            HeaderValue::from_static("A"),
        );
        Ok(())
    }
}

#[derive(Debug)]
struct InjectorB;

impl HeaderInjector for InjectorB {
    fn inject(&self, headers: &mut HeaderMap) -> HttpResult<()> {
        headers.insert(
            HeaderName::from_static("x-seq"),
            HeaderValue::from_static("B"),
        );
        Ok(())
    }
}

#[derive(Debug)]
struct FailingInjector;

impl HeaderInjector for FailingInjector {
    fn inject(&self, _headers: &mut HeaderMap) -> HttpResult<()> {
        Err(HttpError::other("inject failed"))
    }
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
    let client = HttpClientFactory::new().create(options).unwrap();

    let request = client
        .request(Method::GET, format!("{}absolute", target_server.base_url()))
        .build();
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
async fn test_header_injector_order_is_stable_and_clear_works() {
    let server1 = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server1.base_url());
    let client = HttpClientFactory::new().create(options).unwrap();
    client.add_header_injector(Arc::new(InjectorA));
    client.add_header_injector(Arc::new(InjectorB));

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
    let client2 = HttpClientFactory::new().create(options2).unwrap();
    client2.add_header_injector(Arc::new(InjectorA));
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
    let client = HttpClientFactory::new().create(options).unwrap();
    client.add_header_injector(Arc::new(FailingInjector));

    let request = client.request(Method::GET, "/will-not-send").build();
    let error = client.execute(request).await.unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("inject failed"));
}
