/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use context::CoreContext;
use fbinit::FacebookInit;
use mononoke_macros::mononoke;
use tests_utils::BasicTestRepo;
use tokio::runtime::Builder;
use tokio::runtime::RuntimeFlavor;
use tokio::sync::oneshot;

use super::metadata;
use super::run_metadata_computation;
use crate::types::DiffInputString;
use crate::types::DiffSingleInput;

const DEADLOCK_TIMEOUT: Duration = Duration::from_secs(10);

type OnCompute = Box<dyn FnOnce() + Send>;

thread_local! {
    static ON_COMPUTE: RefCell<Option<OnCompute>> = const { RefCell::new(None) };
}

pub(super) fn take_on_compute() -> Option<OnCompute> {
    ON_COMPUTE.with_borrow_mut(Option::take)
}

struct Reservation {
    live: Arc<AtomicBool>,
    dropped: Option<oneshot::Sender<()>>,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.live.store(false, Ordering::SeqCst);
        if let Some(dropped) = self.dropped.take() {
            let _ = dropped.send(());
        }
    }
}

#[mononoke::fbinit_test]
fn metadata_computation_yields_to_current_thread_runtime(fb: FacebookInit) {
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("create current-thread runtime");
    runtime.block_on(async {
        assert_eq!(
            runtime.handle().runtime_flavor(),
            RuntimeFlavor::CurrentThread
        );
        let runtime_thread = thread::current().id();
        let ctx = CoreContext::test_mock(fb);
        let repo: BasicTestRepo = test_repo_factory::build_empty(fb)
            .await
            .expect("create test repo");
        let (started_tx, started_rx) = oneshot::channel();
        let (release_tx, release_rx) = mpsc::channel();

        ON_COMPUTE.with_borrow_mut(|slot| {
            assert!(slot.is_none());
            *slot = Some(Box::new(move || {
                started_tx
                    .send(thread::current().id())
                    .expect("notify the runtime task");
                // Only the async peer can release this controlled CPU-work stand-in.
                // The timeout prevents an inline-work regression from hanging the test.
                release_rx
                    .recv_timeout(DEADLOCK_TIMEOUT)
                    .expect("metadata computation blocked its async peer");
            }));
        });
        let base = DiffSingleInput::String(DiffInputString {
            content: "old\n".to_owned(),
        });
        let other = DiffSingleInput::String(DiffInputString {
            content: "new\n".to_owned(),
        });
        let computation = metadata(&ctx, Some((base, &repo)), Some((other, &repo)), false);
        let peer = async move {
            let computation_thread = tokio::time::timeout(DEADLOCK_TIMEOUT, started_rx)
                .await
                .expect("metadata should enter its CPU phase")
                .expect("CPU phase should notify the peer");
            let _ = release_tx.send(());
            computation_thread
        };

        let (result, computation_thread) = tokio::join!(computation, peer);
        let result = result.expect("metadata computation should complete");
        let counts = result.lines_count.expect("text inputs have line counts");
        assert_eq!(counts.added_lines, 1);
        assert_eq!(counts.deleted_lines, 1);
        assert_ne!(computation_thread, runtime_thread);
    });
}

#[mononoke::test]
async fn metadata_cancellation_keeps_reservation_until_computation_finishes() {
    let live = Arc::new(AtomicBool::new(true));
    let (dropped_tx, dropped_rx) = oneshot::channel();
    let guard = Reservation {
        live: live.clone(),
        dropped: Some(dropped_tx),
    };
    let (started_tx, started_rx) = oneshot::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let request = mononoke::spawn_task(run_metadata_computation(guard, move || {
        started_tx.send(()).expect("notify the awaiting request");
        release_rx
            .recv_timeout(DEADLOCK_TIMEOUT)
            .expect("test should release the computation");
    }));

    started_rx.await.expect("computation should start");
    request.abort();
    assert!(
        request
            .await
            .expect_err("request should be cancelled")
            .is_cancelled()
    );
    assert!(
        live.load(Ordering::SeqCst),
        "running work lost its reservation"
    );

    release_tx.send(()).expect("release the computation");
    tokio::time::timeout(DEADLOCK_TIMEOUT, dropped_rx)
        .await
        .expect("computation should finish")
        .expect("reservation should signal its release");
    assert!(!live.load(Ordering::SeqCst));
}

#[mononoke::test]
async fn metadata_panic_returns_error_and_releases_reservation() {
    let live = Arc::new(AtomicBool::new(true));
    let guard = Reservation {
        live: live.clone(),
        dropped: None,
    };
    let result =
        run_metadata_computation::<()>(guard, || panic!("test metadata computation panic")).await;

    let error = result.expect_err("blocking-task panic should propagate as an error");
    assert!(
        error
            .to_string()
            .contains("metadata diff blocking task failed")
    );
    assert!(!live.load(Ordering::SeqCst));
}

#[mononoke::test]
fn metadata_unpolled_computation_releases_reservation() {
    let live = Arc::new(AtomicBool::new(true));
    let guard = Reservation {
        live: live.clone(),
        dropped: None,
    };
    let request = run_metadata_computation::<()>(guard, || panic!("unpolled work must not run"));
    drop(request);
    assert!(!live.load(Ordering::SeqCst));
}
