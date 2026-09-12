use bytes::Bytes;
use http::HeaderMap;

/// Bounded raw response data retained for a non-success HTTP status.
#[derive(Clone)]
pub struct HttpStatusResponse {
    headers: HeaderMap,
    body: Bytes,
    truncated: bool,
    content_length: Option<u64>,
}

impl HttpStatusResponse {
    /// Creates a bounded status response snapshot.
    pub(crate) fn new(headers: HeaderMap, body: Bytes, truncated: bool, content_length: Option<u64>) -> Self {
        Self {
            headers,
            body,
            truncated,
            content_length,
        }
    }
    /// Returns response headers.
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }
    /// Returns the retained raw body prefix.
    pub fn body(&self) -> &Bytes {
        &self.body
    }
    /// Returns whether the retained body is incomplete.
    pub fn is_truncated(&self) -> bool {
        self.truncated
    }
    /// Returns the complete body length advertised by the server, when known.
    pub fn content_length(&self) -> Option<u64> {
        self.content_length
    }
}

impl std::fmt::Debug for HttpStatusResponse {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HttpStatusResponse")
            .field("body_len", &self.body.len())
            .field("truncated", &self.truncated)
            .field("content_length", &self.content_length)
            .finish()
    }
}
