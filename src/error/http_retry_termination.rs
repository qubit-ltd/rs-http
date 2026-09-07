/// Terminal condition of an HTTP retry flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpRetryTermination {
    /// The retry rule stopped after an attempt.
    Aborted,
    /// The configured attempt limit was reached.
    AttemptsExhausted,
    /// A retry duration budget was reached.
    DurationExceeded,
    /// The operation was cancelled.
    Cancelled,
}
