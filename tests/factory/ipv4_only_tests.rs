/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use std::time::Duration;

use http::Method;
use qubit_http::{HttpClientFactory, HttpClientOptions};
use tokio::time::timeout;

use crate::common::{spawn_one_shot_server, ResponsePlan};

#[test]
fn test_ipv4_only_option_is_preserved_in_client_options() {
    let mut options = HttpClientOptions::default();
    options.ipv4_only = true;
    let client = HttpClientFactory::new().create_with_options(options).unwrap();
    assert!(client.options().ipv4_only);
}

#[tokio::test]
async fn test_ipv4_only_with_localhost_request_is_accessible() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ipv4-only-ok".to_vec(),
    })
    .await;

    let mut localhost_url = server.base_url();
    localhost_url
        .set_host(Some("localhost"))
        .expect("failed to set localhost host");

    let mut options = HttpClientOptions::default();
    options.base_url = Some(localhost_url);
    options.ipv4_only = true;
    options.timeouts.write_timeout = Duration::from_secs(2);
    options.timeouts.read_timeout = Duration::from_secs(2);

    let client = HttpClientFactory::new().create_with_options(options).unwrap();
    let request = client.request(Method::GET, "/ipv4-check").build();
    let response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    assert_eq!(response.status.as_u16(), 200);
    assert_eq!(response.text().unwrap(), "ipv4-only-ok");
}
