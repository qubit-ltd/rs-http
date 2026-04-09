/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use http::StatusCode;
use qubit_http::{HttpError, HttpErrorKind};

#[test]
fn test_http_error_builder_methods() {
    let url = url::Url::parse("https://example.com/test").unwrap();
    let error = HttpError::new(HttpErrorKind::Decode, "decode failure")
        .with_method(http::Method::POST)
        .with_url(url.clone())
        .with_status(StatusCode::BAD_GATEWAY);

    assert_eq!(error.kind, HttpErrorKind::Decode);
    assert_eq!(error.method, Some(http::Method::POST));
    assert_eq!(error.url, Some(url));
    assert_eq!(error.status, Some(StatusCode::BAD_GATEWAY));
    assert!(error.message.contains("decode failure"));
}
