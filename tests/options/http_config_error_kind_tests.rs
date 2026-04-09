/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use qubit_http::HttpConfigErrorKind;

#[test]
fn test_http_config_error_kind_display() {
    assert_eq!(
        HttpConfigErrorKind::MissingField.to_string(),
        "missing field"
    );
    assert_eq!(HttpConfigErrorKind::TypeError.to_string(), "type error");
    assert_eq!(
        HttpConfigErrorKind::InvalidValue.to_string(),
        "invalid value"
    );
    assert_eq!(
        HttpConfigErrorKind::InvalidHeader.to_string(),
        "invalid header"
    );
    assert_eq!(HttpConfigErrorKind::ConfigError.to_string(), "config error");
}
