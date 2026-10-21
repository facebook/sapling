/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! A synchronous bridge that parks a private Tokio runtime on the calling
//! thread (`block_in_place(|| runtime.block_on(..))`) can consume the wake that
//! `FuturesUnordered` sends its parent. If that parent is the future of an outer
//! `block_on`, a later child completion then finds no parent waker and is
//! stranded. Running the work through `run_on_task` makes its parent a Tokio
//! task waker, which a nested parker cannot consume.

use std::future::Future;
use std::future::poll_fn;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::mpsc;
use std::task::Poll;
use std::task::Waker;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use anyhow::Result;
use anyhow::anyhow;
use futures::FutureExt;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::channel::oneshot;
use futures::future::BoxFuture;
use futures::stream;
use mononoke_macros::mononoke;

use super::run_on_task;

enum Event {
    WorkPending(usize),
    BridgeEntered,
    BCompleted,
}

#[derive(Debug)]
struct Outcome {
    step: Result<(), &'static str>,
    poll_with_bridge: usize,
    polls_at_b: Option<usize>,
}

struct Shared {
    work_polls: AtomicUsize,
    work_waker: Mutex<Option<Waker>>,
    finished: AtomicBool,
}

/// Upper bound for B to arrive on its own; success returns as soon as it does.
const B_DEADLINE: Duration = Duration::from_secs(5);

/// A valid yield: wakes itself once, then completes on the next poll.
fn yield_once() -> impl Future<Output = ()> {
    let mut yielded = false;
    poll_fn(move |cx| {
        if yielded {
            Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    })
}

/// Test stand-in for a third-party synchronous bridge: parks a private runtime
/// on the calling thread.
fn nested_parker_block_on<F: Future>(runtime: &tokio::runtime::Runtime, future: F) -> F::Output {
    tokio::task::block_in_place(|| runtime.block_on(future))
}

fn wait_for(
    events: &mpsc::Receiver<Event>,
    deadline: Instant,
    mut matches: impl FnMut(&Event) -> bool,
) -> bool {
    loop {
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return false;
        };
        match events.recv_timeout(remaining) {
            Ok(event) if matches(&event) => return true,
            Ok(_) => {}
            Err(_) => return false,
        }
    }
}

fn wake_work(shared: &Shared) {
    let waker = shared.work_waker.lock().expect("work waker lock").clone();
    if let Some(waker) = waker {
        waker.wake();
    }
}

/// Wraps the observed WORK (inside the boundary), passing the original
/// `Context` through unchanged while counting polls and saving its waker.
fn observe<F>(
    work: F,
    shared: Arc<Shared>,
    events: mpsc::Sender<Event>,
) -> impl Future<Output = F::Output> + Send + 'static
where
    F: Future + Send + 'static,
{
    let mut work = Box::pin(work);
    poll_fn(move |cx| {
        let n = shared.work_polls.fetch_add(1, SeqCst) + 1;
        *shared.work_waker.lock().expect("work waker lock") = Some(cx.waker().clone());
        let polled = work.as_mut().poll(cx);
        if polled.is_pending() {
            let _ = events.send(Event::WorkPending(n));
        }
        polled
    })
}

fn canceled(_: oneshot::Canceled) -> anyhow::Error {
    anyhow!("test signal dropped")
}

fn run_two_completion_sequence() -> (Result<()>, Outcome) {
    let outer = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("outer runtime");
    // Owned here so its last drop happens outside any runtime context.
    let bridge = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("bridge runtime"),
    );
    let shared = Arc::new(Shared {
        work_polls: AtomicUsize::new(0),
        work_waker: Mutex::new(None),
        finished: AtomicBool::new(false),
    });
    let (events_tx, events_rx) = mpsc::channel::<Event>();
    let (a_go_tx, a_go_rx) = oneshot::channel::<()>();
    let (a2_tx, a2_rx) = oneshot::channel::<()>();
    let (b1_tx, b1_rx) = oneshot::channel::<()>();
    let (b2_tx, b2_rx) = oneshot::channel::<()>();
    let (c_tx, c_rx) = oneshot::channel::<()>();
    let (release_tx, release_rx) = oneshot::channel::<()>();

    let a: BoxFuture<'static, Result<()>> = {
        let events = events_tx.clone();
        let bridge = Arc::clone(&bridge);
        async move {
            a_go_rx.await.map_err(canceled)?;
            // Called synchronously from this child's poll.
            nested_parker_block_on(&bridge, async move {
                let _ = events.send(Event::BridgeEntered);
                let _ = release_rx.await;
                yield_once().await;
            });
            a2_rx.await.map_err(canceled)
        }
        .boxed()
    };
    let b: BoxFuture<'static, Result<()>> = {
        let events = events_tx.clone();
        async move {
            b1_rx.await.map_err(canceled)?;
            b2_rx.await.map_err(canceled)?;
            let _ = events.send(Event::BCompleted);
            Ok(())
        }
        .boxed()
    };
    let c: BoxFuture<'static, Result<()>> = async move { c_rx.await.map_err(canceled) }.boxed();

    let driver = {
        let shared = Arc::clone(&shared);
        mononoke::spawn_thread(move || {
            let mut a_go_tx = Some(a_go_tx);
            let mut a2_tx = Some(a2_tx);
            let mut b1_tx = Some(b1_tx);
            let mut b2_tx = Some(b2_tx);
            let mut c_tx = Some(c_tx);
            let mut release_tx = Some(release_tx);
            let mut outcome = Outcome {
                step: Ok(()),
                poll_with_bridge: 0,
                polls_at_b: None,
            };
            let deadline = Instant::now() + Duration::from_secs(20);
            outcome.step = 'steps: {
                if !wait_for(&events_rx, deadline, |e| matches!(e, Event::WorkPending(_))) {
                    break 'steps Err("no initial pending work poll");
                }
                // Every child is pending; wake only A.
                let _ = a_go_tx.take().map(|tx| tx.send(()));
                if !wait_for(&events_rx, deadline, |e| matches!(e, Event::BridgeEntered)) {
                    break 'steps Err("A never entered the nested bridge");
                }
                outcome.poll_with_bridge = shared.work_polls.load(SeqCst);
                // Early completion while A is inside the bridge: enqueues B and
                // takes the parent waker from FuturesUnordered.
                let _ = b1_tx.take().map(|tx| tx.send(()));
                let _ = release_tx.take().map(|tx| tx.send(()));
                let target = outcome.poll_with_bridge;
                if !wait_for(
                    &events_rx,
                    deadline,
                    |e| matches!(e, Event::WorkPending(n) if *n >= target),
                ) {
                    break 'steps Err("work poll containing the bridge never returned");
                }
                // Late completion after that work poll returned Pending. Nothing
                // else wakes the work until B is observed or the deadline passes.
                let _ = b2_tx.take().map(|tx| tx.send(()));
                if wait_for(&events_rx, Instant::now() + B_DEADLINE, |e| {
                    matches!(e, Event::BCompleted)
                }) {
                    outcome.polls_at_b = Some(shared.work_polls.load(SeqCst));
                }
                Ok(())
            };
            // Cleanup regardless of outcome: finish or cancel every child and wake
            // the work until the caller's block_on returns, so a failing shape
            // still terminates.
            let _ = a2_tx.take().map(|tx| tx.send(()));
            let _ = c_tx.take().map(|tx| tx.send(()));
            let _ = b2_tx.take().map(|tx| tx.send(()));
            drop((a_go_tx, b1_tx, release_tx));
            let until = Instant::now() + Duration::from_secs(30);
            while !shared.finished.load(SeqCst) && Instant::now() < until {
                wake_work(&shared);
                thread::sleep(Duration::from_millis(50));
            }
            outcome
        })
    };

    let work = stream::iter(vec![a, b, c])
        .map(Ok::<_, anyhow::Error>)
        .try_for_each_concurrent(None::<usize>, |child| child);
    let observed = observe(work, Arc::clone(&shared), events_tx);
    let result = outer.block_on(run_on_task(observed));
    shared.finished.store(true, SeqCst);
    let outcome = driver.join().expect("driver thread");
    drop(outer);
    drop(bridge);
    (result, outcome)
}

#[mononoke::test]
fn test_run_on_task_delivers_late_completion_despite_nested_parker() {
    for _ in 0..5 {
        let (result, outcome) = run_two_completion_sequence();
        assert!(result.is_ok(), "{result:?} {outcome:?}");
        assert_eq!(outcome.step, Ok(()), "{outcome:?}");
        let polls = outcome.polls_at_b.unwrap_or_else(|| {
            panic!("B's late completion must reach the work without a rescue wake: {outcome:?}")
        });
        assert!(
            polls > outcome.poll_with_bridge,
            "the work must be polled again on its own: {outcome:?}"
        );
    }
}
