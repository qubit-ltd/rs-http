/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use http::header::RETRY_AFTER;
use http::{
    HeaderMap,
    HeaderValue,
    Method,
    StatusCode,
};
use qubit_http::HttpResponseMeta;
use url::Url;

#[test]
fn test_http_response_meta_retry_after_only_applies_to_retryable_statuses() {
    let mut headers = HeaderMap::new();
    headers.insert(RETRY_AFTER, HeaderValue::from_static("7"));
    let url = Url::parse("https://example.com/retry").expect("valid URL");
    let rate_limited = HttpResponseMeta::new(
        StatusCode::TOO_MANY_REQUESTS,
        headers.clone(),
        url.clone(),
        Method::GET,
    );
    let success = HttpResponseMeta::new(StatusCode::OK, headers, url, Method::GET);

    assert_eq!(
        rate_limited.retry_after_hint(),
        Some(std::time::Duration::from_secs(7))
    );
    assert_eq!(success.retry_after_hint(), None);
}
