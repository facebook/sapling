/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use request_context::RequestContext;

/// A snapshot of the calling thread's `folly::RequestContext`, captured so it can
/// be re-installed around blocking work that executes on a different thread.
pub struct CapturedRequestContext(RequestContext);

impl CapturedRequestContext {
    /// Capture the current thread's `RequestContext`, or `None` if none is set.
    pub fn capture() -> Option<Self> {
        RequestContext::try_get_current().map(Self)
    }

    /// Run `func` with the captured `RequestContext` installed on the current
    /// thread, restoring the previous context afterwards, and return its result.
    pub fn run<F, R>(&self, func: F) -> R
    where
        F: FnOnce() -> R,
    {
        let mut out = None;
        self.0.with_context(|| {
            out = Some(func());
        });
        out.expect("with_context always runs the closure unless func panics")
    }
}

/// Install tokio runtime hooks that propagate the ambient `folly::RequestContext`
/// across `tokio::spawn` boundaries, so Artillery trace context survives task
/// hops on the async runtime. Delegates to the shared `request_context_tokio`
/// hooks; call once on the `tokio::runtime::Builder` before `build()`.
pub fn install_request_context_hooks(builder: &mut tokio::runtime::Builder) {
    use request_context_tokio::BuilderExt;
    builder.install_request_context_hooks();
}
