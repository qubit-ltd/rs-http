/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use qubit_http::{
    SensitiveBodyFields,
    SensitiveHttpHeaders,
    SensitiveQueryParams,
    DEFAULT_SENSITIVE_BODY_FIELD_NAMES,
    DEFAULT_SENSITIVE_HEADER_NAMES,
    DEFAULT_SENSITIVE_QUERY_PARAM_NAMES,
};

#[test]
fn test_default_sensitive_name_constants_drive_default_sets() {
    let headers = SensitiveHttpHeaders::default();
    let query_params = SensitiveQueryParams::default();
    let body_fields = SensitiveBodyFields::default();

    for name in DEFAULT_SENSITIVE_HEADER_NAMES {
        assert!(
            headers.contains(name),
            "default header set should contain {name}"
        );
    }
    for name in DEFAULT_SENSITIVE_QUERY_PARAM_NAMES {
        assert!(
            query_params.contains(name),
            "default query set should contain {name}"
        );
    }
    for name in DEFAULT_SENSITIVE_BODY_FIELD_NAMES {
        assert!(
            body_fields.contains(name),
            "default body set should contain {name}"
        );
    }
}

#[test]
fn test_default_sensitive_name_constants_cover_common_http_secrets() {
    assert!(DEFAULT_SENSITIVE_HEADER_NAMES.contains(&"authorization"));
    assert!(DEFAULT_SENSITIVE_HEADER_NAMES.contains(&"set_cookie"));
    assert!(DEFAULT_SENSITIVE_QUERY_PARAM_NAMES.contains(&"access_token"));
    assert!(DEFAULT_SENSITIVE_QUERY_PARAM_NAMES.contains(&"client_secret"));
    assert!(DEFAULT_SENSITIVE_BODY_FIELD_NAMES.contains(&"password"));
    assert!(DEFAULT_SENSITIVE_BODY_FIELD_NAMES.contains(&"refresh_token"));
}
