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

use qubit_http::{
    HttpConfigErrorKind,
    HttpTimeoutOptions,
};

#[test]
fn test_http_timeout_options_validate_rejects_zero_read_timeout() {
    let options = HttpTimeoutOptions {
        read_timeout: Duration::ZERO,
        ..HttpTimeoutOptions::default()
    };

    let error = options.validate().expect_err("zero read timeout should be rejected");

    assert_eq!(error.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(error.path, "read_timeout");
}
