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
    pub use mononoke_proc_macros::test;
    use tokio::task;
    use tracing::Instrument;
    use tracing::Span;

    pub fn override_just_knobs() {
        let just_knobs_json = include_str!("../just_knobs_defaults/just_knobs.json");
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
