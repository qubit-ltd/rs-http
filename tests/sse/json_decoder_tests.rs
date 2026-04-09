/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Tests for `src/sse/json_decoder.rs`.

use bytes::Bytes;
use futures_util::StreamExt;
use http::HeaderMap;
use qubit_http::sse::{
    decode_json_chunks, decode_json_chunks_with_mode, DoneMarkerPolicy, SseChunk, SseJsonMode,
};
use qubit_http::{HttpResult, HttpStreamResponse};

#[derive(Debug, serde::Deserialize, PartialEq, Eq)]
struct TestChunk {
    value: i32,
}

async fn collect_results<T>(stream: impl futures_util::Stream<Item = HttpResult<T>>) -> Vec<T> {
    stream
        .map(|item| item.expect("unexpected stream error in test"))
        .collect::<Vec<_>>()
        .await
}

fn stream_response_from_chunks(chunks: Vec<&'static str>) -> HttpStreamResponse {
    let stream = futures_util::stream::iter(
        chunks
            .into_iter()
            .map(|text| Ok::<Bytes, qubit_http::HttpError>(Bytes::from(text.to_string()))),
    );
    HttpStreamResponse::new(
        http::StatusCode::OK,
        HeaderMap::new(),
        url::Url::parse("https://example.com/stream").unwrap(),
        Box::pin(stream),
    )
}

#[tokio::test]
async fn test_decode_json_chunks_lenient_skips_bad_json_and_respects_done() {
    let response = stream_response_from_chunks(vec![
        "data: {\"value\": 1}\n\n",
        "data: malformed-json\n\n",
        "data: [DONE]\n\n",
        "data: {\"value\": 9}\n\n",
    ]);
    let chunks = collect_results(decode_json_chunks::<TestChunk>(
        response,
        DoneMarkerPolicy::DefaultDone,
    ))
    .await;

    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0], SseChunk::Data(TestChunk { value: 1 }));
    assert_eq!(chunks[1], SseChunk::Done);
}

#[tokio::test]
async fn test_decode_json_chunks_strict_fails_on_bad_json() {
    let response =
        stream_response_from_chunks(vec!["data: {\"value\": 1}\n\n", "data: malformed-json\n\n"]);
    let mut stream = decode_json_chunks_with_mode::<TestChunk>(
        response,
        DoneMarkerPolicy::DefaultDone,
        SseJsonMode::Strict,
    );

    let first = stream.next().await.unwrap().unwrap();
    assert_eq!(first, SseChunk::Data(TestChunk { value: 1 }));

    let second = stream.next().await.unwrap();
    let error = second.unwrap_err();
    assert_eq!(error.kind, qubit_http::HttpErrorKind::SseDecode);
}

#[tokio::test]
async fn test_decode_json_chunks_with_custom_done_marker() {
    let response = stream_response_from_chunks(vec!["data: {\"value\": 2}\n\n", "data: <END>\n\n"]);
    let chunks = collect_results(decode_json_chunks::<TestChunk>(
        response,
        DoneMarkerPolicy::Custom("<END>".to_string()),
    ))
    .await;

    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0], SseChunk::Data(TestChunk { value: 2 }));
    assert_eq!(chunks[1], SseChunk::Done);
}
