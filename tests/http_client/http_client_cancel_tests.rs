/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use qubit_http::{HttpError, HttpErrorKind, RetryHint};

#[test]
fn test_cancelled_error_semantics() {
    let error = HttpError::cancelled("request cancelled by caller");
    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert_eq!(error.retry_hint(), RetryHint::NonRetryable);
    assert!(error.message.contains("cancelled"));
}
