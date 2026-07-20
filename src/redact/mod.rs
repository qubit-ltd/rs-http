// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Immutable HTTP log redaction policies and rendering helpers.

mod body_preview;
mod log_redaction_policy;
mod log_redaction_policy_builder;
mod log_redactor;
mod redacted_debugger;
mod redacted_logger;

pub(crate) use body_preview::BodyPreview;
pub use log_redaction_policy::LogRedactionPolicy;
pub use log_redaction_policy_builder::LogRedactionPolicyBuilder;
pub use log_redactor::LogRedactor;
pub(crate) use redacted_debugger::RedactedDebugger;
pub(crate) use redacted_logger::RedactedLogger;
