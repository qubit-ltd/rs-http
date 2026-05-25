/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use std::time::Duration;

use http::Method;
use qubit_http::{
    HttpClientFactory,
    HttpClientOptions,
    HttpErrorKind,
};
use tokio::time::timeout;

use crate::common::{
    spawn_one_shot_server,
    ResponsePlan,
};

#[tokio::test]
async fn test_backend_error_mapper_classifies_body_read_timeout() {
    let server = spawn_one_shot_server(ResponsePlan::PartialThenDelay {
        status: 200,
        headers: vec![],
        total_length: 16,
        prefix: b"partial".to_vec(),
        delay: Duration::from_secs(2),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_millis(25);
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");
    let mut response = client
        .execute(client.request(Method::GET, "/read-timeout").build())
        .await
        .expect("headers should be received before body timeout");

    let error = response
        .bytes()
        .await
        .expect_err("delayed body should exceed read timeout");

    assert_eq!(error.kind, HttpErrorKind::ReadTimeout);
    assert_eq!(error.method, Some(Method::GET));
    assert_eq!(error.url, Some(server.base_url().join("/read-timeout").unwrap()));

    timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
}
