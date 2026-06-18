/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub mod mononoke {
    use std::future::Future;
    use std::thread;

    use justknobs::test_helpers;
    use justknobs::test_helpers::JustKnobsInMemory;
    pub use mononoke_proc_macros::fbinit_test;
    pub use mononoke_proc_macros::quickcheck_test;
    pub use mononoke_proc_macros::test;
    use request_context_ext::CapturedRequestContext;
    use tokio::task;
    use tracing::Instrument;
    use tracing::Span;

    pub fn override_just_knobs() {
        let just_knobs_json = include_str!("../test_just_knobs/just_knobs.json");
        test_helpers::override_just_knobs(
            JustKnobsInMemory::from_json(just_knobs_json).expect("failed to parse just knobs json"),
        );
    }

    pub fn spawn_task<F>(future: F) -> task::JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        task::spawn(future.in_current_span())
    }

    /// Like `tokio::task::spawn_blocking`, but preserves the caller's tracing span
    /// and (in `fbcode_build`) the ambient `folly::RequestContext`.
    pub fn spawn_blocking<F, R>(func: F) -> task::JoinHandle<R>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        let span = Span::current();
        let captured = CapturedRequestContext::capture();
        task::spawn_blocking(move || {
            let _span_guard = span.enter();
            match captured {
                Some(ctx) => ctx.run(func),
                None => func(),
            }
        })
    }

    pub fn spawn_thread<F, Output>(func: F) -> thread::JoinHandle<Output>
    where
        F: FnOnce() -> Output + Send + 'static,
        Output: Send + 'static,
    {
        let current_span = Span::current();
        thread::spawn(move || {
            let _ = current_span.enter();
            func()
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use tracing::Span;
    use tracing_subscriber::Registry;

    use super::mononoke;

    fn init_tracing() {
        static INIT: OnceLock<()> = OnceLock::new();
        INIT.get_or_init(|| {
            // Registry tracks span IDs without any output.  Ignore the error
            // when another test already set the global default.
            tracing::subscriber::set_global_default(Registry::default()).ok();
        });
    }

    #[tokio::test]
    async fn spawn_blocking_runs_and_returns() {
        let result = mononoke::spawn_blocking(|| 40 + 2)
            .await
            .expect("spawn_blocking task should not panic");
        assert_eq!(
            result, 42,
            "spawn_blocking should run the closure and return its value"
        );
    }

    #[tokio::test]
    async fn spawn_blocking_propagates_span() {
        init_tracing();
        let span = tracing::info_span!("outer");
        let outer_id = span.id().clone();
        // Guard against a vacuous pass: if a subscriber wasn't installed (another
        // test set the global default first), both ids are None and the assert
        // below would trivially hold without exercising propagation.
        assert!(
            outer_id.is_some(),
            "tracing subscriber must assign a span id for this test to be meaningful"
        );
        // Enter the span before calling spawn_blocking so it is captured as current;
        // drop the guard before .await to avoid holding an entered span across a yield point.
        let handle = {
            let _enter = span.enter();
            mononoke::spawn_blocking(|| Span::current().id())
        };
        let inner_id = handle.await.expect("task should not panic");
        assert_eq!(
            inner_id, outer_id,
            "spawn_blocking propagates the caller's tracing span into the blocking thread"
        );
    }
}
