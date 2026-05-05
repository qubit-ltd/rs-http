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
async fn test_reqwest_error_phase_send_timeout_maps_to_write_timeout() {
    let server = spawn_one_shot_server(ResponsePlan::DelayedStart {
        delay: Duration::from_secs(2),
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.write_timeout = Duration::from_millis(25);
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let error = client
        .execute(client.request(Method::GET, "/send-timeout").build())
        .await
        .expect_err("delayed first response should exceed send timeout");

    assert_eq!(error.kind, HttpErrorKind::WriteTimeout);
    assert_eq!(error.method, Some(Method::GET));

    timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
}
