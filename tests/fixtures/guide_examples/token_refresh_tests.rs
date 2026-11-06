// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(test)]
use std::future::Future;
#[cfg(test)]
use std::pin::pin;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
#[cfg(test)]
use std::task::Poll;

use tokio::sync::Semaphore;

static CALLS: AtomicUsize = AtomicUsize::new(0);
static BLOCK: AtomicBool = AtomicBool::new(false);
static FAIL_NEXT: AtomicBool = AtomicBool::new(false);
static STARTED: Semaphore = Semaphore::const_new(0);
static RELEASE: Semaphore = Semaphore::const_new(0);

async fn refresh_access_token() -> HttpResult<String> {
    CALLS.fetch_add(1, Ordering::SeqCst);
    STARTED.add_permits(1);
    if BLOCK.load(Ordering::SeqCst) {
        RELEASE.acquire().await.unwrap().forget();
    }
    if FAIL_NEXT.swap(false, Ordering::SeqCst) {
        return Err(HttpError::other("controlled refresh failure"));
    }
    Ok("test-token".to_owned())
}

#[cfg(test)]
fn reset() {
    let _ = example_refresh_access_token;
    CALLS.store(0, Ordering::SeqCst);
    BLOCK.store(false, Ordering::SeqCst);
    FAIL_NEXT.store(false, Ordering::SeqCst);
    while let Ok(permit) = STARTED.try_acquire() {
        permit.forget();
    }
    while let Ok(permit) = RELEASE.try_acquire() {
        permit.forget();
    }
}

#[tokio::test]
async fn test_concurrent_cold_cache_requests_share_one_refresh() {
    reset();
    BLOCK.store(true, Ordering::SeqCst);
    let provider = std::sync::Arc::new(TokenProvider::new());
    let first_provider = std::sync::Arc::clone(&provider);
    let first = tokio::spawn(async move { first_provider.bearer_token().await });
    STARTED.acquire().await.unwrap().forget();

    let second = provider.bearer_token();
    let mut second = pin!(second);
    futures_util::future::poll_fn(|cx| {
        let _ = Future::poll(second.as_mut(), cx);
        Poll::Ready(())
    })
    .await;
    RELEASE.add_permits(2);

    assert_eq!(first.await.unwrap().unwrap(), "test-token");
    assert_eq!(second.await.unwrap(), "test-token");
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_fresh_cache_does_not_refresh() {
    reset();
    let provider = TokenProvider::new();
    *provider.cached.write().await = Some(CachedToken {
        value: "cached".to_owned(),
        expires_at: Instant::now() + Duration::from_secs(60),
    });
    assert_eq!(provider.bearer_token().await.unwrap(), "cached");
    assert_eq!(CALLS.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn test_expired_cache_refreshes_once() {
    reset();
    let provider = TokenProvider::new();
    *provider.cached.write().await = Some(CachedToken {
        value: "expired".to_owned(),
        expires_at: Instant::now() - Duration::from_secs(1),
    });
    assert_eq!(provider.bearer_token().await.unwrap(), "test-token");
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_failed_refresh_is_not_cached_and_a_later_call_recovers() {
    reset();
    FAIL_NEXT.store(true, Ordering::SeqCst);
    let provider = TokenProvider::new();
    assert!(provider.bearer_token().await.is_err());
    assert!(provider.cached.read().await.is_none());
    assert_eq!(provider.bearer_token().await.unwrap(), "test-token");
    assert_eq!(CALLS.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn test_cancelled_refresh_releases_refresh_lock() {
    reset();
    BLOCK.store(true, Ordering::SeqCst);
    let provider = std::sync::Arc::new(TokenProvider::new());
    let pending_provider = std::sync::Arc::clone(&provider);
    let pending = tokio::spawn(async move { pending_provider.bearer_token().await });
    STARTED.acquire().await.unwrap().forget();
    pending.abort();
    assert!(pending.await.unwrap_err().is_cancelled());

    BLOCK.store(false, Ordering::SeqCst);
    assert_eq!(provider.bearer_token().await.unwrap(), "test-token");
    assert_eq!(CALLS.load(Ordering::SeqCst), 2);
}
