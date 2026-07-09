/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Sync work scheduling primitives for Sapling.
//!
//! Sapling is mostly synchronous Rust, but several latency-sensitive paths benefit from doing
//! independent work in the background or in parallel. This crate centralizes that policy so call
//! sites do not each invent their own queues, thread thresholds, batching, and cancellation rules.
//!
//! The core ideas are:
//!
//! - [`background`] is for speculative or delayed-use work. It always submits the task immediately,
//!   then returns a [`Background`] handle whose `get` methods wait only if the task has already
//!   started elsewhere. If the task is still queued, `get` runs it inline instead of waiting behind
//!   busy workers.
//! - [`Items`] is the common transport type. It is either immediately ready or a stream, so APIs can
//!   return the cheap inline result for small/local work and transparently switch to streaming when
//!   work is produced asynchronously.
//! - `Items` transports batches, not individual values. Callers can still iterate item-by-item for
//!   compatibility, but performance-sensitive pipelines should preserve batches with
//!   [`Items::into_batches`].
//! - [`Work`] starts inline and promotes to executor workers only after fan-out makes
//!   parallelism worthwhile. This keeps small operations cheap without losing throughput for large
//!   dynamic traversals.
//! - `Items` composes: the output of one [`Work`] call can feed another.
//! - `Items` error events are data: consumers decide whether `Err(E)` stops iteration. `Work`
//!   callback errors and channel failures are control-plane failures and cancel the work set.
//!
//! Basic ready-vs-stream usage:
//!
//! ```rust
//! # use slex::Items;
//! let local: Items<i32> = Items::ready(vec![1, 2, 3]);
//! let streamed: Items<i32> = Items::stream([Ok(vec![1, 2]), Ok(vec![3])].into_iter());
//! ```
//!
//! Mapping work while allowing the executor to decide whether to stay inline:
//!
//! ```rust
//! # use slex::{Items, Work, WorkOptions};
//! let input: Items<i32> = Items::ready(vec![1, 2, 3]);
//! let results = Work::map(WorkOptions::new(), input, |item| item * 2);
//! ```
//!
//! Dynamic fan-out uses [`WorkShape::batch`] and [`WorkScope`] to publish results and submit more
//! work from inside a worker:
//!
//! ```rust
//! # use slex::{Items, Work, WorkOptions, WorkScope, WorkShape};
//! let roots: Items<i32, ()> = Items::ready(vec![0]);
//! let _results = Work::run(
//!     WorkOptions::new(),
//!     roots,
//!     WorkShape::batch(
//!         |batch: Result<Vec<i32>, ()>, scope: &mut WorkScope<'_, i32, i32, ()>| {
//!             for item in batch? {
//!                 scope.send_result([item]);
//!                 if item < 3 {
//!                     scope.submit_work(item + 1);
//!                 }
//!             }
//!             Ok(())
//!         },
//!     ),
//! );
//! ```

pub mod channel;
mod items_writer;
mod work;

use std::panic;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

pub use items_writer::ItemsWriter;
pub use items_writer::ItemsWriterOptions;
use parking_lot::Mutex;
pub use slex_items::Batch;
pub use slex_items::DEFAULT_INPUT_BATCH_SIZE;
pub use slex_items::Items;
pub use slex_items::ItemsBatches;
pub use slex_items::ScopedItems;
use tokio::task::JoinHandle;
pub use work::FoldScope;
pub use work::Work;
pub use work::WorkOptions;
pub use work::WorkScope;
pub use work::WorkShape;
#[doc(hidden)]
pub use work::WorkShapeImpl;

struct LimitedSpawner {
    available: AtomicUsize,
}

impl LimitedSpawner {
    fn new(limit: usize) -> Self {
        Self {
            available: AtomicUsize::new(limit.max(1)),
        }
    }

    fn maybe_spawn_one<T, J>(&'static self, min: usize, job: J) -> Option<JoinHandle<T>>
    where
        T: Send + 'static,
        J: FnOnce() -> T + Send + 'static,
    {
        let (claimed, count) = self.claim_count(min, 1);
        if count == 0 {
            return None;
        }
        let permit = (claimed > 0).then_some(LimitedSpawnPermit { spawner: self });
        Some(async_runtime::spawn_blocking(move || {
            let _permit = permit;
            job()
        }))
    }

    fn claim_count(&self, min: usize, max: usize) -> (usize, usize) {
        let max = max.max(min);
        let claimed = self.claim(max);
        // `min` workers are allowed to exceed the soft limit. Work uses this to guarantee forward
        // progress when all permits are occupied; background uses it to start the primary worker.
        (claimed, claimed.max(min).min(max))
    }

    fn spawn_claimed<T, J, F>(
        &'static self,
        claimed: usize,
        count: usize,
        mut make_job: F,
    ) -> Vec<JoinHandle<T>>
    where
        T: Send + 'static,
        J: FnOnce() -> T + Send + 'static,
        F: FnMut() -> J,
    {
        (0..count)
            .map(|index| {
                let permit = (index < claimed).then_some(LimitedSpawnPermit { spawner: self });
                let job = make_job();
                async_runtime::spawn_blocking(move || {
                    let _permit = permit;
                    job()
                })
            })
            .collect()
    }

    fn claim(&self, want: usize) -> usize {
        let mut claimed = 0;
        self.available
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |available| {
                claimed = available.min(want);
                Some(available - claimed)
            })
            .expect("limited spawner budget claim should not fail");
        claimed
    }

    fn release(&self, count: usize) {
        self.available.fetch_add(count, Ordering::Release);
    }
}

struct LimitedSpawnPermit {
    spawner: &'static LimitedSpawner,
}

impl Drop for LimitedSpawnPermit {
    fn drop(&mut self) {
        self.spawner.release(1);
    }
}

/// Handle for an eagerly submitted background computation.
///
/// The task is submitted before this handle is returned; `Background` is not lazy. If the task is
/// still queued when [`Background::get`] or [`Background::get_mut`] needs the value, the caller
/// steals the closure and runs it inline. If a worker already started the task, the caller waits
/// for that worker. [`Background::is_ready`] only checks for completion and never runs work inline
/// or waits for another thread that is forcing the result.
///
/// Panics from the background task are preserved and resumed in whichever caller first forces the
/// result.
pub struct Background<T> {
    state: Arc<BackgroundState<T>>,
    // The oneshot receiver is consumed by whichever path forces the value first. The mutex is the
    // initialization gate between `get`, `get_mut`, and `is_ready`; the computed value itself is
    // stored in `value` after the receiver has been consumed.
    receiver: Mutex<Option<crossfire::oneshot::RxOneshot<std::thread::Result<T>>>>,
    value: OnceLock<T>,
}

/// Submit work to the blocking executor and return a handle that can later force the result.
///
/// Submission is eager: by the time this function returns, the task has either started or is queued
/// in the background executor.
pub fn background<T, F>(name: &'static str, f: F) -> Background<T>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let _ = name;
    let (sender, receiver) = crossfire::oneshot::oneshot();
    let state = Arc::new(BackgroundState {
        task: Mutex::new(BackgroundTask {
            task: Some(Box::new(move || panic::catch_unwind(AssertUnwindSafe(f)))),
            sender: Some(sender),
        }),
    });
    let job_state = Arc::clone(&state);
    background_executor().submit(Box::new(move || job_state.run_once()));
    Background {
        state,
        receiver: Mutex::new(Some(receiver)),
        value: OnceLock::new(),
    }
}

impl<T: Send + 'static> Background<T> {
    /// Wait for the task if needed and return the computed value.
    pub fn get(&self) -> &T {
        self.load(true);
        self.value
            .get()
            .expect("background task should be ready after wait")
    }

    /// Wait for the task if needed and return a mutable computed value.
    pub fn get_mut(&mut self) -> &mut T {
        self.load(true);
        self.value
            .get_mut()
            .expect("background task should be ready after wait")
    }

    /// Return whether the background task has completed without blocking.
    ///
    /// This may return false while another thread is currently forcing the result.
    pub fn is_ready(&self) -> bool {
        self.load(false)
    }

    /// Wait for the task if needed and consume the handle.
    pub fn into_inner(self) -> T {
        self.load(true);
        self.value
            .into_inner()
            .expect("background task should be ready after wait")
    }

    fn load(&self, wait: bool) -> bool {
        if self.value.get().is_some() {
            return true;
        }

        if wait {
            self.state.run_once();
        }

        let mut receiver = if wait {
            self.receiver.lock()
        } else {
            let Some(receiver) = self.receiver.try_lock() else {
                return self.value.get().is_some();
            };
            receiver
        };
        if self.value.get().is_some() {
            return true;
        }

        let mut rx = receiver
            .take()
            .expect("background receiver missing before value was set");
        let result = match wait {
            true => rx
                .recv()
                .unwrap_or_else(|_| panic!("background task exited without returning a value")),
            false => match rx.try_recv() {
                Ok(result) => result,
                Err(crossfire::TryRecvError::Empty) => {
                    *receiver = Some(rx);
                    return false;
                }
                Err(crossfire::TryRecvError::Disconnected) => {
                    panic!("background task exited without returning a value")
                }
            },
        };

        let value = match result {
            Ok(value) => value,
            Err(payload) => panic::resume_unwind(payload),
        };
        self.value
            .set(value)
            .unwrap_or_else(|_| panic!("background task was forced reentrantly"));
        true
    }
}

struct BackgroundState<T> {
    task: Mutex<BackgroundTask<T>>,
}

struct BackgroundTask<T> {
    task: Option<Box<dyn FnOnce() -> std::thread::Result<T> + Send + 'static>>,
    sender: Option<crossfire::oneshot::TxOneshot<std::thread::Result<T>>>,
}

impl<T> BackgroundState<T> {
    fn run_once(&self) {
        // Both the queued worker and a forcing caller race through this path. The winner takes the
        // closure and sender; the loser observes `None` and exits/waits without doing duplicate
        // work. The user closure runs outside the mutex.
        let Some((task, sender)) = ({
            let mut state = self.task.lock();
            state.task.take().zip(state.sender.take())
        }) else {
            return;
        };

        sender.send(task());
    }
}

type BackgroundJob = Box<dyn FnOnce() + Send + 'static>;
type BackgroundSender = crossfire::MTx<crossfire::mpmc::List<BackgroundJob>>;
type BackgroundReceiver = crossfire::MRx<crossfire::mpmc::List<BackgroundJob>>;

struct BackgroundExecutor {
    sender: BackgroundSender,
    receiver: BackgroundReceiver,
    primary_started: AtomicBool,
    spawner: LimitedSpawner,
}

impl BackgroundExecutor {
    fn submit(&'static self, job: BackgroundJob) {
        self.sender
            .send(job)
            .unwrap_or_else(|_| panic!("background executor stopped"));
        // The primary worker is lazy but persistent: it is created by the first submitted job and
        // then blocks on `recv()` so later background submissions do not need to spawn a thread.
        if !self.primary_started.swap(true, Ordering::AcqRel) {
            self.spawn_worker(true);
        }
    }

    fn spawn_worker(&'static self, primary: bool) {
        let min = usize::from(primary);
        let receiver = self.receiver.clone();
        let _ = self
            .spawner
            .maybe_spawn_one(min, move || self.run_worker(receiver, primary));
    }

    fn run_worker(&'static self, receiver: BackgroundReceiver, primary: bool) {
        loop {
            // The primary worker blocks and stays alive. Extra workers only drain already-queued
            // work with `try_recv()` and exit as soon as the queue is empty.
            let Some(job) = (if primary {
                receiver.recv().ok()
            } else {
                receiver.try_recv().ok()
            }) else {
                return;
            };

            // Grow the pool lazily under backlog. Each worker only starts one more worker, bounded
            // by `LimitedSpawner`, so a burst ramps up without eagerly creating all workers for one
            // small task.
            if !receiver.is_empty() {
                self.spawn_worker(false);
            }

            job();
        }
    }
}

static BACKGROUND_EXECUTOR: OnceLock<BackgroundExecutor> = OnceLock::new();
const BACKGROUND_WORKERS: usize = 8;

fn background_executor() -> &'static BackgroundExecutor {
    BACKGROUND_EXECUTOR.get_or_init(|| {
        let (sender, receiver) = crossfire::mpmc::unbounded_blocking::<BackgroundJob>();
        BackgroundExecutor {
            sender,
            receiver,
            primary_started: AtomicBool::new(false),
            spawner: LimitedSpawner::new(BACKGROUND_WORKERS),
        }
    })
}

/// Join a blocking task from sync or async context.
///
/// Calling this inside an async context requires Tokio's multi-thread runtime because it uses
/// `block_in_place`.
pub(crate) fn join_blocking<T>(handle: JoinHandle<T>) -> T {
    let result = if tokio::runtime::Handle::try_current().is_ok() {
        tokio::task::block_in_place(|| async_runtime::block_on(handle))
    } else {
        async_runtime::block_on(handle)
    };
    match result {
        Ok(value) => value,
        Err(err) if err.is_panic() => panic::resume_unwind(err.into_panic()),
        Err(err) => panic!("sapling executor task was cancelled: {err}"),
    }
}

const DEFAULT_MAX_WORKERS: usize = 8;

pub(crate) fn default_max_workers() -> usize {
    num_cpus::get().clamp(1, DEFAULT_MAX_WORKERS)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use super::*;

    #[test]
    fn background_spawn_can_be_mutated_after_force() {
        let mut handle = background("test", || 41);

        *handle.get_mut() += 1;
        assert_eq!(handle.into_inner(), 42);
    }

    #[test]
    fn background_always_starts_on_executor() {
        let caller = thread::current().id();
        let (sender, receiver) = mpsc::channel();
        let handle = background("test", move || {
            let thread_id = thread::current().id();
            sender.send(thread_id).unwrap();
            42
        });

        let worker = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_ne!(worker, caller);
        assert_eq!(*handle.get(), 42);
    }

    #[test]
    fn background_get_runs_unstarted_task_inline() {
        let caller = thread::current().id();
        let (sender, receiver) = crossfire::oneshot::oneshot();
        let handle = Background {
            state: Arc::new(BackgroundState {
                task: Mutex::new(BackgroundTask {
                    task: Some(Box::new(|| Ok(thread::current().id()))),
                    sender: Some(sender),
                }),
            }),
            receiver: Mutex::new(Some(receiver)),
            value: OnceLock::new(),
        };

        assert_eq!(*handle.get(), caller);
    }

    #[test]
    fn background_get_can_block_inside_runtime() {
        async_runtime::block_on(async {
            let handle = background("test", || 42);

            assert_eq!(*handle.get(), 42);
        });
    }

    #[test]
    fn background_is_ready_does_not_block() {
        let handle = background("test", || {
            std::thread::sleep(std::time::Duration::from_millis(20));
            42
        });

        assert!(!handle.is_ready());
        assert_eq!(*handle.get(), 42);
        assert!(handle.is_ready());
    }

    #[test]
    fn background_is_ready_does_not_block_on_receiver_lock() {
        let handle = Arc::new(background("test", || 42));
        let probe = Arc::clone(&handle);
        let _forcing_result = handle.receiver.lock();
        let (sender, receiver) = mpsc::channel();

        thread::spawn(move || {
            sender.send(probe.is_ready()).unwrap();
        });

        assert!(!receiver.recv_timeout(Duration::from_secs(1)).unwrap());
    }

    #[test]
    fn items_ready_avoids_stream_setup() {
        let batch: Items<i32> = Items::ready(vec![1, 2, 3]);
        let items = batch.into_iter().collect::<Result<Vec<_>, _>>().unwrap();

        assert_eq!(items, vec![1, 2, 3]);
    }

    #[test]
    fn items_ready_accepts_stack_collections() {
        let batch: Items<i32> = Items::ready(&[1, 2, 3][..]);
        let items = batch.into_iter().collect::<Result<Vec<_>, _>>().unwrap();

        assert_eq!(items, vec![1, 2, 3]);
    }

    #[test]
    fn fallible_items_flatten_without_nested_result() {
        let batch: Items<i32, &'static str> = Items::stream([Ok(vec![1]), Err("boom")].into_iter());
        let mut iter = batch.into_iter();

        assert_eq!(iter.next(), Some(Ok(1)));
        assert_eq!(iter.next(), Some(Err("boom")));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn item_stream_can_be_returned_as_boxed_item_iter() {
        let batch: Items<i32, &'static str> =
            Items::item_stream(vec![Ok(1), Err("boom")].into_iter());
        let mut iter = Box::new(batch.into_iter());

        assert_eq!(iter.next(), Some(Ok(1)));
        assert_eq!(iter.next(), Some(Err("boom")));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn item_stream_into_batches_coalesces_items() {
        let batch: Items<i32> = Items::item_stream((0..3).map(Ok));
        let batches = batch.into_batches().collect::<Result<Vec<_>, _>>().unwrap();

        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].as_slice(), &[0, 1, 2]);
    }

    #[test]
    fn item_stream_into_batches_preserves_fallible_items() {
        let batch: Items<i32, &'static str> =
            Items::item_stream(vec![Ok(1), Ok(2), Err("boom"), Ok(3)].into_iter());
        let mut batches = batch.into_batches();

        assert_eq!(batches.next().unwrap().unwrap().as_slice(), &[1, 2]);
        assert_eq!(batches.next(), Some(Err("boom")));
        assert_eq!(batches.next().unwrap().unwrap().as_slice(), &[3]);
    }

    #[test]
    fn items_writer_inline_finishes_as_ready_items() {
        let mut writer = ItemsWriter::<i32>::inline();
        assert!(writer.push_item(1));
        assert!(writer.push_item(3));

        assert_eq!(writer.finish_inline(), vec![1, 3]);
    }

    #[test]
    fn items_writer_stream_flushes_batches() {
        let options = ItemsWriterOptions::new().batch_items(2).queue_size(2);
        let (mut writer, items) = ItemsWriter::<i32>::stream_with_options(options);

        assert!(writer.push_item(1));
        assert!(writer.push_item(2));
        assert!(writer.push_item(3));
        assert!(writer.close());

        let mut batches = items.into_batches().collect::<Vec<_>>();
        assert_eq!(batches.len(), 2);
        assert_eq!(batches.remove(0).unwrap().as_slice(), &[1, 2]);
        assert_eq!(batches.remove(0).unwrap().as_slice(), &[3]);
    }

    #[test]
    fn items_writer_inline_can_emit_errors_between_batches() {
        let mut writer = ItemsWriter::<i32, &'static str>::inline();
        assert!(writer.push_item(1));
        assert!(writer.push_error("boom"));
        assert!(writer.push_item(2));

        let mut batches = writer.finish().into_batches();
        assert_eq!(batches.next().unwrap().unwrap().as_slice(), &[1]);
        assert_eq!(batches.next(), Some(Err("boom")));
        assert_eq!(batches.next().unwrap().unwrap().as_slice(), &[2]);
        assert_eq!(batches.next(), None);
    }

    #[test]
    fn items_writer_stream_can_emit_errors_between_batches() {
        let options = ItemsWriterOptions::new().batch_items(2).queue_size(3);
        let (mut writer, items) = ItemsWriter::<i32, &'static str>::stream_with_options(options);

        assert!(writer.push_item(1));
        assert!(writer.push_error("boom"));
        assert!(writer.push_item(2));
        assert!(writer.close());

        let mut batches = items.into_batches();
        assert_eq!(batches.next().unwrap().unwrap().as_slice(), &[1]);
        assert_eq!(batches.next(), Some(Err("boom")));
        assert_eq!(batches.next().unwrap().unwrap().as_slice(), &[2]);
        assert_eq!(batches.next(), None);
    }

    #[test]
    fn items_from_process_inline_returns_ready_items() {
        let items: Items<i32> = ItemsWriter::from_process(false, |writer| {
            assert!(writer.push_item(1));
            assert!(writer.push_item(2));
        });

        assert!(matches!(items.into_batches(), ItemsBatches::Ready(_)));
    }

    #[test]
    fn items_from_process_inline_returns_expected_items() {
        let items: Items<i32> = ItemsWriter::from_process(false, |writer| {
            assert!(writer.push_item(1));
            assert!(writer.push_item(2));
        });

        assert_eq!(
            items.into_iter().collect::<Result<Vec<_>, _>>().unwrap(),
            vec![1, 2]
        );
    }

    #[test]
    fn items_from_process_spawn_returns_stream() {
        let caller = thread::current().id();
        let items: Items<thread::ThreadId> = ItemsWriter::from_process(true, move |writer| {
            assert!(writer.push_item(thread::current().id()));
        });

        let ids = items.into_iter().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(ids.len(), 1);
        assert_ne!(ids[0], caller);
    }
}
