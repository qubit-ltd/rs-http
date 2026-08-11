// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::sync::Arc;
use std::sync::Mutex;

use tracing_subscriber::fmt::MakeWriter;

#[derive(Clone)]
struct SharedWriter {
    buffer: Arc<Mutex<Vec<u8>>>,
}

impl<'a> MakeWriter<'a> for SharedWriter {
    type Writer = SharedWriterGuard;

    fn make_writer(&'a self) -> Self::Writer {
        SharedWriterGuard {
            buffer: self.buffer.clone(),
        }
    }
}

struct SharedWriterGuard {
    buffer: Arc<Mutex<Vec<u8>>>,
}

impl io::Write for SharedWriterGuard {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut guard = self
            .buffer
            .lock()
            .expect("failed to acquire tracing capture mutex");
        guard.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub fn capture_trace_logs<F>(func: F) -> String
where
    F: FnOnce(),
{
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let writer = SharedWriter {
        buffer: buffer.clone(),
    };

    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .without_time()
        .with_ansi(false)
        .with_writer(writer)
        .finish();

    tracing::subscriber::with_default(subscriber, func);

    let bytes = buffer
        .lock()
        .expect("failed to read tracing capture buffer")
        .clone();
    String::from_utf8(bytes).expect("captured tracing output is not valid UTF-8")
}
