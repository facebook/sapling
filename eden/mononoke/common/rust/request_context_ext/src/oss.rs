/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

/// No-op stand-in for OSS builds, where there is no `folly::RequestContext`.
///
/// [`capture`](Self::capture) always returns `None`, so [`run`](Self::run) is
/// never reached; it exists only so callers compile unchanged in OSS builds.
pub struct CapturedRequestContext;

impl CapturedRequestContext {
    /// Always `None` in OSS builds (there is no `folly::RequestContext`).
    pub fn capture() -> Option<Self> {
        None
    }

    /// Runs `func` directly; there is no context to install in OSS builds.
    pub fn run<F, R>(&self, func: F) -> R
    where
        F: FnOnce() -> R,
    {
        func()
    }
}

/// No-op in OSS builds — there is no `folly::RequestContext` to propagate.
pub fn install_request_context_hooks(_builder: &mut tokio::runtime::Builder) {}

pub fn with_fresh_request_context<F: std::future::Future>(
    fut: F,
) -> impl std::future::Future<Output = F::Output> {
    fut
}
