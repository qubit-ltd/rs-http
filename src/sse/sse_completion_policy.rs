/// Controls whether a JSON SSE stream may end at ordinary EOF.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum SseCompletionPolicy {
    /// Accept ordinary EOF after the last complete event.
    #[default]
    AllowEof,
    /// Require a matching done marker before EOF.
    RequireDoneMarker,
}
