/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use qubit_http::BodyLogContext;

#[test]
fn test_body_log_context_debug_and_copy() {
    let context = BodyLogContext::ErrorResponse;
    let copied = context;

    assert_eq!(context, copied);
    assert_eq!(format!("{context:?}"), "ErrorResponse");
}
