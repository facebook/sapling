/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::future::Future;
use std::io;
use std::panic::AssertUnwindSafe;
use std::panic::panic_any;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use futures::FutureExt;
use mononoke_macros::mononoke;
#[cfg(fbcode_build)]
use request_context::RequestContext;
use tokio::runtime::Builder;
use tokio::sync::oneshot;
use tokio::time::timeout;
use tokio_util::task::AbortOnDropHandle;
use tracing::Dispatch;
use tracing::Span;
use tracing_subscriber::Registry;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;

use super::run_on_task;

const TEST_TIMEOUT: Duration = Duration::from_secs(30);
const EVENT_TIMEOUT: Duration = Duration::from_secs(10);

fn runtime_builder() -> Builder {
    let mut builder = Builder::new_multi_thread();
    builder.worker_threads(2).enable_all();
    // The same adapter used by MononokeAppBuilder::create_runtime.
    #[cfg(fbcode_build)]
    request_context_ext::install_request_context_hooks(&mut builder);
    builder
}

async fn bounded<F: Future>(future: F) -> F::Output {
    timeout(TEST_TIMEOUT, future)
        .await
        .expect("task-boundary test must finish within its deadline")
}

fn run_bounded<F: Future>(future: F) -> F::Output {
    let runtime = runtime_builder().build().expect("test runtime");
    runtime.block_on(bounded(future))
}

#[mononoke::test]
fn task_boundary_returns_success() {
    run_bounded(run_on_task(async { Ok(()) })).expect("inner success must be returned");
}

#[mononoke::test]
fn task_boundary_preserves_inner_error() {
    let error = run_bounded(run_on_task(async {
        Err(io::Error::new(io::ErrorKind::PermissionDenied, "cgdm inner error").into())
    }))
    .expect_err("inner error must not become success");

    assert_eq!(error.to_string(), "cgdm inner error");
    assert_eq!(
        error
            .downcast_ref::<io::Error>()
            .expect("original error type")
            .kind(),
        io::ErrorKind::PermissionDenied,
    );
}

#[derive(Debug)]
struct PanicMarker(Arc<()>);

#[mononoke::test]
fn task_boundary_resumes_original_panic_payload() {
    let marker = Arc::new(());
    let expected = Arc::clone(&marker);
    let outcome = run_bounded(
        AssertUnwindSafe(run_on_task(async move { panic_any(PanicMarker(marker)) })).catch_unwind(),
    );
    let payload = outcome.expect_err("task panic must resume unwinding on the caller");
    let marker = payload
        .downcast::<PanicMarker>()
        .expect("original panic payload type");
    assert!(
        Arc::ptr_eq(&marker.0, &expected),
        "preserve the original payload value"
    );
}

struct SignalOnDrop(Option<oneshot::Sender<()>>);

impl Drop for SignalOnDrop {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

#[mononoke::test]
fn task_boundary_eventually_drops_inner_future_when_caller_is_aborted() {
    run_bounded(async {
        let (started_tx, started_rx) = oneshot::channel();
        let (dropped_tx, dropped_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel::<()>();
        let caller = AbortOnDropHandle::new(mononoke::spawn_task(run_on_task(async move {
            let _drop_signal = SignalOnDrop(Some(dropped_tx));
            let _ = started_tx.send(());
            let _ = release_rx.await;
            Ok(())
        })));

        timeout(EVENT_TIMEOUT, started_rx)
            .await
            .expect("inner future must start")
            .expect("inner future sent its start signal");
        caller.abort();
        let error = timeout(EVENT_TIMEOUT, caller)
            .await
            .expect("caller cancellation must finish")
            .expect_err("the caller was aborted while its inner future was pending");
        assert!(error.is_cancelled());

        // Aborting the caller schedules child cancellation; destruction is not immediate.
        timeout(EVENT_TIMEOUT, dropped_rx)
            .await
            .expect("inner future must eventually be dropped, not detached")
            .expect("inner future sent its drop signal");
        // Also releases a mistakenly detached child when a preceding assertion unwinds.
        drop(release_tx);
    });
}

#[mononoke::test]
fn task_boundary_preserves_current_tracing_span_across_yield() {
    // Observe the caller span without Tokio's TRACE-level runtime/resource spans.
    let dispatch = Dispatch::new(Registry::default().with(LevelFilter::INFO));
    tracing::dispatcher::with_default(&dispatch, || {
        let span = tracing::info_span!("cgdm_task_boundary_test");
        let expected = span.id().expect("subscriber must assign a real span id");
        let caller_thread = thread::current().id();
        let worker_dispatch = dispatch.clone();
        // This synchronous scope stays on the block_on caller; it is not moved to a worker.
        let _entered = span.enter();
        run_bounded(run_on_task(async move {
            assert_ne!(thread::current().id(), caller_thread);
            // Supply only the subscriber for observation, never an entered worker span.
            // Removing spawn_task's in_current_span must make the id comparison fail.
            let before =
                tracing::dispatcher::with_default(&worker_dispatch, || Span::current().id());
            assert_eq!(before, Some(expected.clone()));
            tokio::task::yield_now().await;
            let after =
                tracing::dispatcher::with_default(&worker_dispatch, || Span::current().id());
            assert_eq!(after, Some(expected));
            Ok(())
        }))
        .expect("traced task must finish");
    });
}

#[cfg(fbcode_build)]
struct RestoreRequestContext(Option<RequestContext>);

#[cfg(fbcode_build)]
impl Drop for RestoreRequestContext {
    fn drop(&mut self) {
        let _ = RequestContext::swap_current(self.0.as_ref());
    }
}

#[cfg(fbcode_build)]
fn current_root_id() -> Option<isize> {
    RequestContext::try_get_current().map(|context| context.get_root_id())
}

#[cfg(fbcode_build)]
#[mononoke::test]
fn task_boundary_preserves_some_request_context_across_yield() {
    let _restore = RestoreRequestContext(RequestContext::swap_current(None));
    RequestContext::create();
    let context = RequestContext::get_current();
    let expected = Some(context.get_root_id());
    let caller_thread = thread::current().id();

    run_bounded(run_on_task(async move {
        assert_ne!(thread::current().id(), caller_thread);
        assert_eq!(current_root_id(), expected);
        tokio::task::yield_now().await;
        assert_eq!(current_root_id(), expected);
        Ok(())
    }))
    .expect("context-carrying task must finish");
    assert_eq!(
        current_root_id(),
        expected,
        "caller context stays installed"
    );
}

#[cfg(fbcode_build)]
#[mononoke::test]
fn task_boundary_preserves_none_over_a_workers_ambient_request_context() {
    let _restore = RestoreRequestContext(RequestContext::swap_current(None));
    let mut builder = runtime_builder();
    // Make None propagation non-vacuous: every worker starts with a different ambient context.
    builder.on_thread_start(|| {
        RequestContext::create();
    });
    let runtime = builder.build().expect("context-hooked test runtime");
    let caller_thread = thread::current().id();
    runtime
        .block_on(bounded(run_on_task(async move {
            assert_ne!(thread::current().id(), caller_thread);
            assert_eq!(current_root_id(), None);
            tokio::task::yield_now().await;
            assert_eq!(current_root_id(), None);
            Ok(())
        })))
        .expect("context-free task must finish");
    assert_eq!(current_root_id(), None, "caller must remain context-free");
}
