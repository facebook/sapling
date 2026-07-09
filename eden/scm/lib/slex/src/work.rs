/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! `Work` models work on [`Items`]: it transforms one batched `Items` stream into another.
//!
//! Small ready inputs run inline on the caller thread so cheap operations do not pay task,
//! synchronization, or channel overhead. Once queued work is large enough to keep multiple workers
//! busy, `Work` promotes to blocking-executor workers. Promotion is bounded by a process-wide soft
//! worker limit so one large traversal does not create an unbounded number of threads.
//!
//! `Work` is batch based. Inputs and outputs are `Vec<T>` batches, not individual items. This keeps
//! channel sends, queue bookkeeping, and callback overhead proportional to batches while preserving
//! item-by-item compatibility through `Items` iterators.
//!
//! Basic per-item mapping:
//!
//! ```rust
//! # use slex::{Items, Work, WorkOptions};
//! let input: Items<i32> = Items::ready(vec![1, 2, 3]);
//! let output = Work::map(WorkOptions::new(), input, |item| item * 2);
//! ```
//!
//! Dynamic batch workers can publish results and submit more work:
//!
//! ```rust
//! # use slex::{Items, Work, WorkOptions, WorkShape};
//! let input = Items::ready(vec![0usize]);
//! let output = Work::run(
//!     WorkOptions::new(),
//!     input,
//!     WorkShape::batch(|batch, scope| -> Result<(), ()> {
//!         let batch = batch?;
//!         for item in batch {
//!             scope.send_result([item]);
//!             if item < 3 {
//!                 scope.submit_work(item + 1);
//!             }
//!         }
//!         Ok(())
//!     }),
//! );
//! ```
//!
//! Batch workers can also keep per-worker local state and reduce it after all work completes. This
//! avoids a shared mutex in the hot worker path:
//!
//! ```rust
//! # use slex::{Items, Work, WorkOptions, WorkScope, WorkShape};
//! let input = Items::ready((0..16usize).collect::<Vec<_>>());
//! let output = Work::run(
//!     WorkOptions::new().inline_items(1),
//!     input,
//!     WorkShape::batch_finalize(
//!         || 0usize,
//!         |batch: Result<Vec<usize>, ()>, scope: &mut WorkScope<'_, usize, String, (), usize>| {
//!             let batch = batch?;
//!             let even_count = batch.iter().filter(|item| **item % 2 == 0).count();
//!             *scope.local_mut() += even_count;
//!
//!             scope.send_result(batch.into_iter().map(|item| format!("visited {item}")));
//!             Ok(())
//!         },
//!         |locals| {
//!             let even_count = locals.into_iter().sum::<usize>();
//!             Ok(Some(vec![format!("even items: {even_count}")]))
//!         },
//!     ),
//! );
//! ```

use std::collections::VecDeque;
use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use parking_lot::Condvar;
use parking_lot::Mutex;
use slex_items::Batch as ItemsBatch;
use slex_items::DEFAULT_INPUT_BATCH_SIZE;
use tokio::task::JoinHandle;

use crate::Items;
use crate::ItemsWriter;
use crate::join_blocking;

/// Dynamically discovered work that starts inline and promotes on fan-out.
///
/// Work is intended for synchronous call sites that may do little work or may recursively discover
/// a large amount of independent work. It avoids spawning for small ready inputs, then promotes to
/// blocking-executor workers once the configured thresholds are crossed.
pub struct Work;

impl Work {
    /// Run a work shape over input items.
    ///
    /// The work shape controls whether this is a simple per-item map, a fallible per-item map, a
    /// dynamic batch expansion, or a batch expansion with worker-local finalization.
    ///
    /// The returned [`Items`] is ready if all work stayed inline. If the work promotes, it is a
    /// stream; dropping that stream cancels unfinished work, while consuming it joins workers.
    ///
    /// Worker callback errors are fatal: they cancel remaining work and are emitted as the returned
    /// `Items` error. Upstream input errors are passed to batch workers as `Err`, so workers can
    /// either propagate them with `?` for fail-fast behavior or forward them with
    /// [`WorkScope::send_error`] for collect-all pipelines.
    pub fn run<W, Out, E, K>(options: WorkOptions, input: Items<W, E>, worker: K) -> Items<Out, E>
    where
        W: Send + 'static,
        Out: Send + 'static,
        E: Send + 'static,
        K: WorkShapeImpl<W, Out, E>,
    {
        run_worker(options, input, worker)
    }

    /// Map each input item to exactly one output item.
    ///
    /// This is the infallible fast path. It avoids per-item `Result` wrapping and checking in the
    /// ready inline path and in worker batches.
    pub fn map<W, Out, E, K>(options: WorkOptions, input: Items<W, E>, f: K) -> Items<Out, E>
    where
        W: Send + 'static,
        Out: Send + 'static,
        E: Send + 'static,
        K: Fn(W) -> Out + Send + Sync + 'static,
    {
        Self::run(options, input, WorkShape::flat(f))
    }

    /// Map each input item to one fallible output item, canceling on the first callback error.
    ///
    /// Use this when item processing can fail and later work is not useful after the first error.
    pub fn try_map<W, Out, E, K>(options: WorkOptions, input: Items<W, E>, f: K) -> Items<Out, E>
    where
        W: Send + 'static,
        Out: Send + 'static,
        E: Send + 'static,
        K: Fn(W) -> Result<Out, E> + Send + Sync + 'static,
    {
        Self::run(options, input, WorkShape::try_flat(f))
    }
}

/// Scheduling options for [`Work`].
///
/// Defaults are tuned to avoid overhead for small work and promote once there is enough queued work
/// to keep more than one worker busy. Most callers should only set `inline_items`, `max_workers`,
/// or result buffering.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkOptions {
    /// Maximum workers this operation may use after promotion.
    pub max_workers: usize,
    /// Stay inline while queued item count is at or below this value.
    pub inline_items: usize,
    /// Number of work items per worker batch.
    pub work_chunk_items: usize,
    /// Keep results inline until this many result items are buffered and work remains.
    pub inline_result_items: usize,
    /// Number of result batches to buffer in streaming mode.
    pub result_queue_size: usize,
    /// Number of output items per streaming result batch.
    pub result_batch_items: usize,
}

impl WorkOptions {
    const DEFAULT_INLINE_ITEMS: usize = DEFAULT_INPUT_BATCH_SIZE;
    const DEFAULT_INLINE_RESULT_ITEMS: usize = 1024;
    const DEFAULT_RESULT_BATCH_ITEMS: usize = 128;
    const DEFAULT_RESULT_QUEUE_SIZE: usize = 8;

    /// Create default work options.
    pub fn new() -> Self {
        Self {
            max_workers: crate::default_max_workers(),
            inline_items: Self::DEFAULT_INLINE_ITEMS,
            work_chunk_items: Self::DEFAULT_INLINE_ITEMS,
            inline_result_items: Self::DEFAULT_INLINE_RESULT_ITEMS,
            result_queue_size: Self::DEFAULT_RESULT_QUEUE_SIZE,
            result_batch_items: Self::DEFAULT_RESULT_BATCH_ITEMS,
        }
    }

    /// Set the maximum worker count. Values below one are clamped to one.
    pub fn max_workers(mut self, max_workers: usize) -> Self {
        self.max_workers = max_workers.max(1);
        self
    }

    /// Set the inline item threshold.
    ///
    /// This also defaults `work_chunk_items` to the same value so most callers tune only one item
    /// count knob.
    pub fn inline_items(mut self, inline_items: usize) -> Self {
        self.inline_items = inline_items;
        self.work_chunk_items = inline_items.max(1);
        self
    }

    /// Override work chunk size independently from `inline_items`.
    pub fn work_chunk_items(mut self, work_chunk_items: usize) -> Self {
        self.work_chunk_items = work_chunk_items.max(1);
        self
    }

    /// Set how many result items may be buffered while still returning ready results.
    pub fn inline_result_items(mut self, inline_result_items: usize) -> Self {
        self.inline_result_items = inline_result_items.max(1);
        self
    }

    /// Set streaming result queue size in batches.
    pub fn result_queue_size(mut self, result_queue_size: usize) -> Self {
        self.result_queue_size = result_queue_size.max(1);
        self
    }

    /// Set streaming result batch size.
    pub fn result_batch_items(mut self, result_batch_items: usize) -> Self {
        self.result_batch_items = result_batch_items.max(1);
        self
    }

    fn should_promote(&self, queued_items: usize) -> bool {
        self.max_workers > 1
            && self.has_parallel_work(queued_items)
            && queued_items > self.inline_items
    }

    fn has_parallel_work(&self, item_count: usize) -> bool {
        item_count >= self.work_chunk_items.saturating_mul(2)
    }
}

impl Default for WorkOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// Constructors for work shapes used by [`Work::run`].
pub struct WorkShape;

/// Work shape for infallible one-to-one item mapping.
pub struct Flat<F>(F);
/// Work shape for fallible one-to-one item mapping.
pub struct TryFlat<F>(F);
/// Work shape for batch callbacks that can publish results and submit more work.
pub struct Batch<W, Out, E, F> {
    f: F,
    _types: PhantomData<fn(W, Out, E)>,
}
/// Batch work shape with per-worker local state and a finalizer.
pub struct BatchFinalize<W, Out, E, F, Init, Finish, Local> {
    f: F,
    init: Init,
    finish: Finish,
    _types: PhantomData<fn(W, Out, E, Local)>,
}

impl WorkShape {
    /// Map each input item to exactly one output item.
    ///
    /// This is the cheapest work shape for pure transformations. Ready inputs stay inline until
    /// the configured promotion threshold is reached.
    pub fn flat<F>(f: F) -> Flat<F> {
        Flat(f)
    }

    /// Map each input item to exactly one output item, canceling on the first error.
    pub fn try_flat<F>(f: F) -> TryFlat<F> {
        TryFlat(f)
    }

    /// Process batches of input work, optionally publishing results and submitting more work.
    ///
    /// Use this for tree walks, graph traversals, and other dynamic fan-out where workers discover
    /// more work as they run.
    pub fn batch<W, Out, E, F>(f: F) -> Batch<W, Out, E, F>
    where
        F: for<'a> Fn(Result<Vec<W>, E>, &mut WorkScope<'a, W, Out, E>) -> Result<(), E>
            + Send
            + Sync
            + 'static,
    {
        Batch {
            f,
            _types: PhantomData,
        }
    }

    /// Process batches with typed per-worker local state and a finalizer.
    ///
    /// Use this when each worker should accumulate local state without shared locking, then reduce
    /// those local states after all work completes.
    /// The finalizer only runs after normal completion; cancellation skips it.
    pub fn batch_finalize<W, Out, E, Init, Local, F, Finish>(
        init: Init,
        f: F,
        finish: Finish,
    ) -> BatchFinalize<W, Out, E, F, Init, Finish, Local>
    where
        Init: Fn() -> Local + Send + Sync + 'static,
        Local: Send + 'static,
        F: for<'a> Fn(Result<Vec<W>, E>, &mut WorkScope<'a, W, Out, E, Local>) -> Result<(), E>
            + Send
            + Sync
            + 'static,
        Finish: Fn(Vec<Local>) -> Result<Option<Vec<Out>>, E> + Send + Sync + 'static,
    {
        BatchFinalize {
            f,
            init,
            finish,
            _types: PhantomData,
        }
    }
}

/// Internal trait implemented by [`WorkShape`] modes.
///
/// Most callers should use [`WorkShape::flat`], [`WorkShape::try_flat`],
/// [`WorkShape::batch`], or [`WorkShape::batch_finalize`] instead of implementing this trait
/// directly.
#[doc(hidden)]
pub trait WorkShapeImpl<W, Out, E>: Send + Sync + 'static
where
    Out: Send + 'static,
    E: Send + 'static,
{
    type Local: Send + 'static;

    /// Initialize per-worker state.
    fn init(&self) -> Self::Local;

    /// Optional fast path for ready inputs that stay inline.
    fn try_inline_ready(&self, work: Vec<W>) -> Result<Result<Vec<Out>, Vec<W>>, E> {
        Ok(Err(work))
    }

    /// Process one batch of work.
    fn process(
        &self,
        batch: Result<Vec<W>, E>,
        scope: &mut WorkScope<'_, W, Out, E, Self::Local>,
    ) -> Result<(), E>;

    /// Finish worker-local state after all work is complete.
    fn finish(&self, _locals: Vec<Self::Local>) -> Result<Option<Vec<Out>>, E> {
        Ok(None)
    }
}

impl<W, Out, E, F> WorkShapeImpl<W, Out, E> for Flat<F>
where
    W: Send + 'static,
    Out: Send + 'static,
    E: Send + 'static,
    F: Fn(W) -> Out + Send + Sync + 'static,
{
    type Local = ();

    fn init(&self) -> Self::Local {}

    fn try_inline_ready(&self, work: Vec<W>) -> Result<Result<Vec<Out>, Vec<W>>, E> {
        Ok(Ok(work.into_iter().map(&self.0).collect()))
    }

    fn process(
        &self,
        batch: Result<Vec<W>, E>,
        scope: &mut WorkScope<'_, W, Out, E, ()>,
    ) -> Result<(), E> {
        let batch = batch?;
        scope.send_result(batch.into_iter().map(&self.0));
        Ok(())
    }
}

impl<W, Out, E, F> WorkShapeImpl<W, Out, E> for TryFlat<F>
where
    W: Send + 'static,
    Out: Send + 'static,
    E: Send + 'static,
    F: Fn(W) -> Result<Out, E> + Send + Sync + 'static,
{
    type Local = ();

    fn init(&self) -> Self::Local {}

    fn try_inline_ready(&self, work: Vec<W>) -> Result<Result<Vec<Out>, Vec<W>>, E> {
        let mut results = Vec::with_capacity(work.len());
        for item in work {
            results.push((self.0)(item)?);
        }
        Ok(Ok(results))
    }

    fn process(
        &self,
        batch: Result<Vec<W>, E>,
        scope: &mut WorkScope<'_, W, Out, E, ()>,
    ) -> Result<(), E> {
        let batch = batch?;
        let mut results = Vec::with_capacity(batch.len());
        for item in batch {
            results.push((self.0)(item)?);
        }
        scope.send_result(results);
        Ok(())
    }
}

impl<W, Out, E, F> WorkShapeImpl<W, Out, E> for Batch<W, Out, E, F>
where
    W: Send + 'static,
    Out: Send + 'static,
    E: Send + 'static,
    F: for<'a> Fn(Result<Vec<W>, E>, &mut WorkScope<'a, W, Out, E>) -> Result<(), E>
        + Send
        + Sync
        + 'static,
{
    type Local = ();

    fn init(&self) -> Self::Local {}

    fn process(
        &self,
        batch: Result<Vec<W>, E>,
        scope: &mut WorkScope<'_, W, Out, E, ()>,
    ) -> Result<(), E> {
        (self.f)(batch, scope)
    }
}

impl<W, Out, E, F, Init, Finish, Local> WorkShapeImpl<W, Out, E>
    for BatchFinalize<W, Out, E, F, Init, Finish, Local>
where
    W: Send + 'static,
    Out: Send + 'static,
    E: Send + 'static,
    Local: Send + 'static,
    Init: Fn() -> Local + Send + Sync + 'static,
    F: for<'a> Fn(Result<Vec<W>, E>, &mut WorkScope<'a, W, Out, E, Local>) -> Result<(), E>
        + Send
        + Sync
        + 'static,
    Finish: Fn(Vec<Local>) -> Result<Option<Vec<Out>>, E> + Send + Sync + 'static,
{
    type Local = Local;

    fn init(&self) -> Self::Local {
        (self.init)()
    }

    fn process(
        &self,
        batch: Result<Vec<W>, E>,
        scope: &mut WorkScope<'_, W, Out, E, Local>,
    ) -> Result<(), E> {
        (self.f)(batch, scope)
    }

    fn finish(&self, locals: Vec<Self::Local>) -> Result<Option<Vec<Out>>, E> {
        (self.finish)(locals)
    }
}

/// Work submitted from a [`WorkScope`].
///
/// This lets producers submit either a single item or an owned/borrowed batch while leaving
/// buffering and chunking to `Work`.
pub trait IntoWorkItems<W> {
    /// Append this input into the scope's pending work buffer.
    fn append_to(self, work: &mut Vec<W>);
}

impl<W> IntoWorkItems<W> for W {
    fn append_to(self, work: &mut Vec<W>) {
        work.push(self);
    }
}

impl<W> IntoWorkItems<W> for Vec<W> {
    fn append_to(self, work: &mut Vec<W>) {
        work.extend(self);
    }
}

impl<W> IntoWorkItems<W> for ItemsBatch<W> {
    fn append_to(self, work: &mut Vec<W>) {
        work.extend(self);
    }
}

impl<W: Clone> IntoWorkItems<W> for &[W] {
    fn append_to(self, work: &mut Vec<W>) {
        work.extend_from_slice(self);
    }
}

impl<W, const N: usize> IntoWorkItems<W> for [W; N] {
    fn append_to(self, work: &mut Vec<W>) {
        work.extend(self);
    }
}

impl<W> IntoWorkItems<W> for std::vec::IntoIter<W> {
    fn append_to(self, work: &mut Vec<W>) {
        work.extend(self);
    }
}

/// Scope passed to batch workers.
///
/// The scope is the only way batch workers communicate with Work: publish output with
/// [`WorkScope::send_result`] or add more work with [`WorkScope::submit_work`].
/// Return `Err` from the worker callback to stop the pipeline with an observable error.
pub struct WorkScope<'a, W, Out: Send + 'static, E: Send + 'static, Local = ()> {
    backend: &'a mut dyn ScopeBackend<W, Out, E>,
    local: &'a mut Local,
    pending_work: Vec<W>,
    result_buffer: Vec<Out>,
    work_chunk_items: usize,
    result_batch_items: usize,
}

impl<W, Out, E, Local> WorkScope<'_, W, Out, E, Local>
where
    Out: Send + 'static,
    E: Send + 'static,
{
    /// Borrow this worker's local state.
    pub fn local(&self) -> &Local {
        self.local
    }

    /// Mutably borrow this worker's local state.
    pub fn local_mut(&mut self) -> &mut Local {
        self.local
    }

    /// Enqueue more work. Returns false if the set has been canceled.
    pub fn submit_work<I>(&mut self, items: I) -> bool
    where
        I: IntoWorkItems<W>,
    {
        if self.is_canceled() {
            return false;
        }
        items.append_to(&mut self.pending_work);
        if self.pending_work.len() >= self.work_chunk_items {
            self.flush_work(true)
        } else {
            !self.is_canceled()
        }
    }

    fn submit_input_error(&mut self, err: E) -> bool {
        if self.is_canceled() || !self.flush_work(true) {
            return false;
        }
        self.backend.submit_error(err)
    }

    /// Publish results. In inline mode this appends to the ready result buffer; after promotion it
    /// sends batches to the bounded result channel.
    pub fn send_result<I>(&mut self, items: I) -> bool
    where
        I: IntoIterator<Item = Out>,
    {
        if self.is_canceled() {
            return false;
        }
        self.result_buffer.extend(items);
        self.flush_ready_results()
    }

    /// Publish a nonfatal error event and continue processing.
    ///
    /// Use callback `Err` for fatal worker failures that should cancel the work. Use this for
    /// result-level errors where the consumer decides whether to keep draining or stop early.
    /// Returns `false` if buffered results or the error could not be published.
    pub fn send_error(&mut self, err: E) -> bool {
        if self.is_canceled() || !self.flush_results(true) {
            return false;
        }
        self.backend.send_error(err)
    }

    /// Whether this work set has been canceled.
    pub fn is_canceled(&self) -> bool {
        self.backend.is_canceled()
    }

    fn finish(&mut self) -> bool {
        self.flush_results(false) && self.flush_work(false)
    }

    fn flush_work(&mut self, retain_buffer: bool) -> bool {
        if self.pending_work.is_empty() {
            return true;
        }
        if self.is_canceled() {
            self.pending_work.clear();
            return false;
        }

        let work = if retain_buffer && self.backend.retain_scope_buffers() {
            self.pending_work.drain(..).collect()
        } else {
            std::mem::take(&mut self.pending_work)
        };

        self.backend.submit_work(work)
    }

    fn flush_ready_results(&mut self) -> bool {
        if self.result_buffer.len() < self.result_batch_items {
            return !self.is_canceled();
        }
        self.flush_results(true)
    }

    fn flush_results(&mut self, retain_buffer: bool) -> bool {
        if self.result_buffer.is_empty() {
            return true;
        }
        if self.is_canceled() {
            self.result_buffer.clear();
            return false;
        }

        let results = if retain_buffer && self.backend.retain_scope_buffers() {
            self.result_buffer.drain(..).collect()
        } else {
            std::mem::take(&mut self.result_buffer)
        };

        self.backend.send_result_batch(results)
    }
}

type ResultSender<Out, E> = crate::channel::Sender<Result<Vec<Out>, E>>;
type ResultReceiver<Out, E> = crate::channel::Receiver<Result<Vec<Out>, E>>;

struct WorkIter<Out: Send + 'static, E: Send + 'static> {
    pending: VecDeque<Result<Vec<Out>, E>>,
    rx: Option<ResultReceiver<Out, E>>,
    cancel: Option<Box<dyn Fn() + Send + Sync>>,
    handles: Vec<JoinHandle<()>>,
    joined: bool,
}

impl<Out, E> WorkIter<Out, E>
where
    Out: Send + 'static,
    E: Send + 'static,
{
    fn new(
        pending: VecDeque<Result<Vec<Out>, E>>,
        rx: ResultReceiver<Out, E>,
        cancel: Box<dyn Fn() + Send + Sync>,
        handles: Vec<JoinHandle<()>>,
    ) -> Self {
        Self {
            pending,
            rx: Some(rx),
            cancel: Some(cancel),
            handles,
            joined: false,
        }
    }

    fn cancel(&self) {
        if let Some(cancel) = &self.cancel {
            cancel();
        }
    }

    fn join(&mut self) {
        if self.joined {
            return;
        }
        for handle in self.handles.drain(..) {
            join_blocking(handle);
        }
        self.cancel = None;
        self.joined = true;
    }

    fn join_suppress_panics(&mut self) {
        if self.joined {
            return;
        }
        for handle in self.handles.drain(..) {
            let _ = async_runtime::block_on(handle);
        }
        self.cancel = None;
        self.joined = true;
    }
}

impl<Out, E> Iterator for WorkIter<Out, E>
where
    Out: Send + 'static,
    E: Send + 'static,
{
    type Item = Result<Vec<Out>, E>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(event) = self.pending.pop_front() {
            return Some(event);
        }
        let rx = self.rx.as_ref()?;
        match rx.recv() {
            Ok(batch) => Some(batch),
            Err(_) => {
                self.rx.take();
                self.join();
                None
            }
        }
    }
}

impl<Out, E> Drop for WorkIter<Out, E>
where
    Out: Send + 'static,
    E: Send + 'static,
{
    fn drop(&mut self) {
        if !self.joined {
            self.cancel();
            self.rx.take();
            self.join_suppress_panics();
        }
    }
}

// The work queue is physically unbounded so worker fanout cannot deadlock. This logical throttle
// only blocks external producers when consumers fall behind.
struct WorkQueueThrottle {
    queued: AtomicUsize,
    limit: usize,
    lock: Mutex<()>,
    changed: Condvar,
}

impl WorkQueueThrottle {
    fn new(limit: usize) -> Self {
        Self {
            queued: AtomicUsize::new(0),
            limit: limit.max(1),
            lock: Mutex::new(()),
            changed: Condvar::new(),
        }
    }

    fn maybe_throttle(&self, canceled: &AtomicBool) -> bool {
        loop {
            if canceled.load(Ordering::Acquire) {
                return false;
            }

            let queued = self.queued.load(Ordering::Acquire);
            if queued < self.limit
                && self
                    .queued
                    .compare_exchange_weak(queued, queued + 1, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
            {
                return true;
            }

            let mut lock = self.lock.lock();
            while self.queued.load(Ordering::Acquire) >= self.limit
                && !canceled.load(Ordering::Acquire)
            {
                self.changed.wait(&mut lock);
            }
        }
    }

    fn track_unthrottled(&self) {
        self.queued.fetch_add(1, Ordering::AcqRel);
    }

    fn unthrottle(&self) {
        let old = self.queued.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(old > 0, "Work queue throttle underflow");
        if old >= self.limit {
            let _lock = self.lock.lock();
            self.changed.notify_one();
        }
    }

    fn wake_all(&self) {
        let _lock = self.lock.lock();
        self.changed.notify_all();
    }
}

trait ScopeBackend<W, Out, E> {
    fn submit_work(&mut self, batch: Vec<W>) -> bool;
    fn submit_error(&mut self, err: E) -> bool;
    fn send_result_batch(&mut self, batch: Vec<Out>) -> bool;
    fn send_error(&mut self, err: E) -> bool;
    fn is_canceled(&self) -> bool;
    fn retain_scope_buffers(&self) -> bool;
}

fn run_worker<W, Out, E, K>(options: WorkOptions, input: Items<W, E>, worker: K) -> Items<Out, E>
where
    W: Send + 'static,
    Out: Send + 'static,
    E: Send + 'static,
    K: WorkShapeImpl<W, Out, E>,
{
    match input.into_batches() {
        crate::ItemsBatches::Ready(mut work) if work.len() == 1 => {
            match work
                .next()
                .expect("ready batch should exist after length check")
            {
                Ok(work) if options.should_promote(work.len()) => {
                    run_inline(options, [Ok(work.into_vec())], worker)
                }
                Ok(work) => match worker.try_inline_ready(work.into_vec()) {
                    Ok(Ok(results)) => Items::ready(results),
                    Ok(Err(work)) => run_inline(options, [Ok(work)], worker),
                    Err(err) => Items::error(err),
                },
                Err(err) => run_inline(options, [Err(err)], worker),
            }
        }
        crate::ItemsBatches::Ready(work) => run_inline(
            options,
            work.map(|batch| batch.map(|batch| batch.into_vec())),
            worker,
        ),
        stream @ crate::ItemsBatches::Stream(_) => {
            Items::stream(run_streaming_batches(options, stream, Arc::new(worker)))
        }
    }
}

fn run_inline<W, Out, E, K, I>(options: WorkOptions, initial: I, worker: K) -> Items<Out, E>
where
    W: Send + 'static,
    Out: Send + 'static,
    E: Send + 'static,
    K: WorkShapeImpl<W, Out, E>,
    I: IntoIterator<Item = Result<Vec<W>, E>>,
{
    let mut inline = InlineState::new();
    for batch in initial {
        inline.submit_input(batch, options.work_chunk_items);
    }
    let mut local = worker.init();

    loop {
        if inline.is_canceled() {
            return inline.finish();
        }

        if !inline.has_queued_work() {
            match worker.finish(vec![local]) {
                Ok(Some(results)) => {
                    inline.send_result_batch(results, options.inline_result_items);
                }
                Ok(None) => {}
                Err(err) => inline.cancel_with_error(err),
            }
            return inline.finish();
        };

        if inline.should_return_stream() || options.should_promote(inline.queued_items) {
            let worker = Arc::new(worker);
            return Items::stream(promote_inline(options, inline, worker, vec![local]));
        }

        let Some(batch) = inline.try_pop() else {
            continue;
        };
        let result = {
            let mut backend = InlineBackend {
                inline: &mut inline,
                work_chunk_items: options.work_chunk_items,
                inline_result_items: options.inline_result_items,
            };
            let mut scope = WorkScope {
                backend: &mut backend,
                local: &mut local,
                pending_work: scope_buffer(options.work_chunk_items),
                result_buffer: scope_buffer(options.result_batch_items),
                work_chunk_items: options.work_chunk_items,
                result_batch_items: options.result_batch_items,
            };
            let result = worker.process(batch, &mut scope);
            if result.is_ok() {
                scope.finish();
            }
            result
        };
        if let Err(err) = result {
            inline.cancel_with_error(err);
        }
    }
}

type RawWorkSender<W, E> = crossfire::MTx<crossfire::mpmc::List<QueuedWork<W, E>>>;
type WorkReceiver<W, E> = crossfire::MRx<crossfire::mpmc::List<QueuedWork<W, E>>>;
type WorkGuard = crossfire::waitgroup::WaitGroupGuard<()>;
type SpawnRestFn = Arc<dyn Fn(usize) + Send + Sync + 'static>;

trait WorkSubmitter<W, E>: Send + Sync {
    fn send_queued(&self, work: QueuedWork<W, E>) -> Result<(), ()>;
}

struct WorkSender<W, E>(Arc<dyn WorkSubmitter<W, E>>);

impl<W, E> Clone for WorkSender<W, E> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<W: Send + 'static, E: Send + 'static> WorkSender<W, E> {
    fn new(sender: RawWorkSender<W, E>) -> Self {
        Self(Arc::new(CrossfireWorkSubmitter { sender }))
    }

    fn send(&self, batch: Vec<W>, guard: WorkGuard) -> Result<(), ()> {
        self.send_result(Ok(batch), guard)
    }

    fn send_error(&self, err: E, guard: WorkGuard) -> Result<(), ()> {
        self.send_result(Err(err), guard)
    }

    fn send_result(&self, batch: Result<Vec<W>, E>, guard: WorkGuard) -> Result<(), ()> {
        self.0.send_queued(QueuedWork {
            batch,
            guard,
            work_tx: self.clone(),
        })
    }
}

struct CrossfireWorkSubmitter<W: Send + 'static, E: Send + 'static> {
    sender: RawWorkSender<W, E>,
}

impl<W: Send + 'static, E: Send + 'static> WorkSubmitter<W, E> for CrossfireWorkSubmitter<W, E> {
    fn send_queued(&self, work: QueuedWork<W, E>) -> Result<(), ()> {
        self.sender.send(work).map_err(|_| ())
    }
}

struct ParallelState<W, Out: Send + 'static, E: Send + 'static, K> {
    worker: Arc<K>,
    result_tx: ResultSender<Out, E>,
    canceled: Arc<AtomicBool>,
    work_queue_throttle: Arc<WorkQueueThrottle>,
    result_batch_items: usize,
    work_chunk_items: usize,
    _work: PhantomData<fn(W)>,
}

impl<W, Out, E, K> Clone for ParallelState<W, Out, E, K>
where
    Out: Send + 'static,
    E: Send + 'static,
{
    fn clone(&self) -> Self {
        Self {
            worker: Arc::clone(&self.worker),
            result_tx: self.result_tx.clone(),
            canceled: Arc::clone(&self.canceled),
            work_queue_throttle: Arc::clone(&self.work_queue_throttle),
            result_batch_items: self.result_batch_items,
            work_chunk_items: self.work_chunk_items,
            _work: PhantomData,
        }
    }
}

impl<W, Out, E, K> ParallelState<W, Out, E, K>
where
    Out: Send + 'static,
    E: Send + 'static,
{
    fn cancel(&self) {
        self.canceled.store(true, Ordering::Release);
        self.work_queue_throttle.wake_all();
    }

    fn cancel_with_error(&self, err: E) {
        if !self.canceled.swap(true, Ordering::AcqRel) {
            self.work_queue_throttle.wake_all();
            let _ = self.result_tx.send(Err(err));
        }
    }
}

fn promote_inline<W, Out, E, K>(
    options: WorkOptions,
    inline: InlineState<W, Out, E>,
    worker: Arc<K>,
    initial_locals: Vec<K::Local>,
) -> WorkIter<Out, E>
where
    W: Send + 'static,
    Out: Send + 'static,
    E: Send + 'static,
    K: WorkShapeImpl<W, Out, E>,
{
    let max_workers = options.max_workers;
    let mut inline = inline;
    let canceled = Arc::new(AtomicBool::new(inline.canceled));
    let work_queue_throttle = Arc::new(WorkQueueThrottle::new(work_queue_throttle_limit(
        max_workers,
    )));
    let (raw_work_tx, work_rx) = crossfire::mpmc::unbounded_blocking();
    let work_tx = WorkSender::new(raw_work_tx);
    let (result_tx, result_rx) = crate::channel::bounded(options.result_queue_size);
    let state = ParallelState {
        worker,
        result_tx,
        canceled,
        work_queue_throttle,
        result_batch_items: options.result_batch_items,
        work_chunk_items: options.work_chunk_items,
        _work: PhantomData,
    };
    let wait = crossfire::waitgroup::WaitGroup::new((), 0);
    let has_work = !inline.queue.is_empty();
    let initial_chunks = inline.queue.len();

    for queued in inline.queue.drain(..) {
        state.work_queue_throttle.track_unthrottled();
        work_tx
            .send_result(queued, wait.add_guard())
            .expect("promoted inline work receiver should be alive");
    }
    let pending_events = inline.take_output_events().into_iter().collect();

    debug_assert!(
        has_work,
        "promote_inline should only be called with queued work"
    );

    let initial_worker_want = initial_worker_count(max_workers, initial_chunks);
    let late_worker_handles = Arc::new(Mutex::new(Vec::new()));
    let spawn_rest_state = state.clone();
    let spawn_rest_work_rx = work_rx.clone();
    let spawn_rest_handles = Arc::clone(&late_worker_handles);
    let (worker_handles, _) = spawn_workers(
        1,
        initial_worker_want,
        work_rx,
        state.clone(),
        move |worker_count| {
            make_spawn_rest(
                worker_count,
                initial_chunks,
                max_workers,
                spawn_rest_work_rx,
                spawn_rest_state,
                spawn_rest_handles,
            )
        },
    );
    let handles = vec![spawn_coordinator(
        wait,
        work_tx,
        initial_locals,
        worker_handles,
        Some(late_worker_handles),
        state.clone(),
    )];

    WorkIter::new(
        pending_events,
        result_rx,
        cancel_closure(
            Arc::clone(&state.canceled),
            Arc::clone(&state.work_queue_throttle),
        ),
        handles,
    )
}

fn run_streaming_batches<W, Out, E, K>(
    options: WorkOptions,
    input: crate::ItemsBatches<'static, W, E>,
    worker: Arc<K>,
) -> WorkIter<Out, E>
where
    W: Send + 'static,
    Out: Send + 'static,
    E: Send + 'static,
    K: WorkShapeImpl<W, Out, E>,
{
    let initial_worker_want = 1;
    let canceled = Arc::new(AtomicBool::new(false));
    let work_queue_throttle = Arc::new(WorkQueueThrottle::new(work_queue_throttle_limit(
        options.max_workers,
    )));
    let (raw_work_tx, work_rx) = crossfire::mpmc::unbounded_blocking();
    let work_tx = WorkSender::new(raw_work_tx);
    let (result_tx, result_rx) = crate::channel::bounded(options.result_queue_size);
    let state = ParallelState {
        worker,
        result_tx,
        canceled,
        work_queue_throttle,
        result_batch_items: options.result_batch_items,
        work_chunk_items: options.work_chunk_items,
        _work: PhantomData,
    };
    let wait = crossfire::waitgroup::WaitGroup::new((), 0);
    let producer_guard = wait.add_guard();
    let late_worker_handles = Arc::new(Mutex::new(Vec::new()));
    let spawn_rest_state = state.clone();
    let spawn_rest_work_rx = work_rx.clone();
    let spawn_rest_handles = Arc::clone(&late_worker_handles);
    let (worker_handles, spawn_rest) = spawn_workers(
        1,
        initial_worker_want,
        work_rx,
        state.clone(),
        move |worker_count| {
            make_spawn_rest(
                worker_count,
                0,
                options.max_workers,
                spawn_rest_work_rx,
                spawn_rest_state,
                spawn_rest_handles,
            )
        },
    );

    let producer_state = state.clone();
    let producer_work_tx = work_tx.clone();
    let producer_handle = work_spawner()
        .maybe_spawn_one(1, move || {
            let mut backend = ParallelBackend {
                work_tx: &producer_work_tx,
                guard: &producer_guard,
                state: producer_state.clone(),
                throttle_producer: true,
                spawn_rest,
            };
            let mut local = ();
            let mut scope: WorkScope<'_, W, Out, E> = WorkScope {
                backend: &mut backend,
                local: &mut local,
                pending_work: scope_buffer(producer_state.work_chunk_items),
                result_buffer: scope_buffer(producer_state.result_batch_items),
                work_chunk_items: producer_state.work_chunk_items,
                result_batch_items: producer_state.result_batch_items,
            };
            if produce_input_work(input, &mut scope) {
                scope.finish();
            }
        })
        .expect("streaming Work producer should spawn");

    let coordinator_handle = spawn_coordinator(
        wait,
        work_tx,
        Vec::new(),
        worker_handles,
        Some(late_worker_handles),
        state.clone(),
    );

    WorkIter::new(
        VecDeque::new(),
        result_rx,
        cancel_closure(
            Arc::clone(&state.canceled),
            Arc::clone(&state.work_queue_throttle),
        ),
        vec![producer_handle, coordinator_handle],
    )
}

fn produce_input_work<W, Out, E>(
    input: crate::ItemsBatches<'static, W, E>,
    scope: &mut WorkScope<'_, W, Out, E>,
) -> bool
where
    W: Send + 'static,
    Out: Send + 'static,
    E: Send + 'static,
{
    for batch in input {
        match batch {
            Ok(batch) => {
                if !scope.submit_work(batch) {
                    return false;
                }
            }
            Err(err) => {
                if !scope.submit_input_error(err) {
                    return false;
                }
            }
        }
    }

    true
}

fn spawn_workers<W, Out, E, K, MakeSpawnRest>(
    min_workers: usize,
    max_workers: usize,
    work_rx: WorkReceiver<W, E>,
    state: ParallelState<W, Out, E, K>,
    make_spawn_rest: MakeSpawnRest,
) -> (Vec<JoinHandle<K::Local>>, Option<SpawnRestFn>)
where
    W: Send + 'static,
    Out: Send + 'static,
    E: Send + 'static,
    K: WorkShapeImpl<W, Out, E>,
    MakeSpawnRest: FnOnce(usize) -> Option<SpawnRestFn>,
{
    let spawner = work_spawner();
    let (claimed, worker_count) = spawner.claim_count(min_workers, max_workers);
    let spawn_rest = make_spawn_rest(worker_count);
    let spawn_rest_for_workers = spawn_rest.clone();
    let handles = spawner.spawn_claimed(claimed, worker_count, move || {
        let work_rx = work_rx.clone();
        let state = state.clone();
        let spawn_rest = spawn_rest_for_workers.clone();
        move || {
            let mut local = state.worker.init();
            while let Ok(queued) = work_rx.recv() {
                state.work_queue_throttle.unthrottle();
                let QueuedWork {
                    batch,
                    guard,
                    work_tx,
                } = queued;
                if state.canceled.load(Ordering::Acquire) {
                    continue;
                }
                let result = {
                    let mut backend = ParallelBackend {
                        work_tx: &work_tx,
                        guard: &guard,
                        state: state.clone(),
                        throttle_producer: false,
                        spawn_rest: spawn_rest.clone(),
                    };
                    let mut scope = WorkScope {
                        backend: &mut backend,
                        local: &mut local,
                        pending_work: scope_buffer(state.work_chunk_items),
                        result_buffer: scope_buffer(state.result_batch_items),
                        work_chunk_items: state.work_chunk_items,
                        result_batch_items: state.result_batch_items,
                    };
                    let result = state.worker.process(batch, &mut scope);
                    if result.is_ok() {
                        scope.finish();
                    }
                    result
                };
                if let Err(err) = result {
                    state.cancel_with_error(err);
                }
            }
            local
        }
    });
    (handles, spawn_rest)
}

const MIN_WORKER_SOFT_LIMIT: usize = 16;
const MAX_WORKER_SOFT_LIMIT: usize = 128;
const WORKER_SOFT_LIMIT_PER_CPU: usize = 2;

fn worker_soft_limit() -> usize {
    num_cpus::get()
        .saturating_mul(WORKER_SOFT_LIMIT_PER_CPU)
        .clamp(MIN_WORKER_SOFT_LIMIT, MAX_WORKER_SOFT_LIMIT)
}

static WORK_SPAWNER: OnceLock<crate::LimitedSpawner> = OnceLock::new();

fn work_spawner() -> &'static crate::LimitedSpawner {
    WORK_SPAWNER.get_or_init(|| crate::LimitedSpawner::new(worker_soft_limit()))
}

fn work_queue_throttle_limit(max_workers: usize) -> usize {
    const PER_WORKER: usize = 16;

    max_workers.max(1).saturating_mul(PER_WORKER)
}

fn initial_worker_count(max_workers: usize, initial_chunks: usize) -> usize {
    max_workers.min(initial_chunks).max(1)
}

fn make_spawn_rest<W, Out, E, K>(
    initial_workers: usize,
    initial_submitted_chunks: usize,
    max_workers: usize,
    work_rx: WorkReceiver<W, E>,
    state: ParallelState<W, Out, E, K>,
    handles: Arc<Mutex<Vec<JoinHandle<K::Local>>>>,
) -> Option<SpawnRestFn>
where
    W: Send + 'static,
    Out: Send + 'static,
    E: Send + 'static,
    K: WorkShapeImpl<W, Out, E>,
{
    if initial_workers >= max_workers {
        return None;
    }

    let submitted_chunks = Arc::new(AtomicUsize::new(initial_submitted_chunks));
    let triggered = Arc::new(AtomicBool::new(false));
    Some(Arc::new(move |new_chunks| {
        let submitted = submitted_chunks.fetch_add(new_chunks, Ordering::AcqRel) + new_chunks;
        if submitted <= initial_workers
            || triggered
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return;
        }

        let (worker_handles, _) = spawn_workers(
            0,
            max_workers.saturating_sub(initial_workers),
            work_rx.clone(),
            state.clone(),
            |_| None,
        );
        handles.lock().extend(worker_handles);
    }))
}

fn spawn_coordinator<W, Out, E, K>(
    wait: crossfire::waitgroup::WaitGroup<()>,
    work_tx: WorkSender<W, E>,
    initial_locals: Vec<K::Local>,
    worker_handles: Vec<JoinHandle<K::Local>>,
    late_worker_handles: Option<Arc<Mutex<Vec<JoinHandle<K::Local>>>>>,
    state: ParallelState<W, Out, E, K>,
) -> JoinHandle<()>
where
    W: Send + 'static,
    Out: Send + 'static,
    E: Send + 'static,
    K: WorkShapeImpl<W, Out, E>,
{
    work_spawner()
        .maybe_spawn_one(1, move || {
            wait.wait();
            drop(work_tx);
            let mut locals = initial_locals;
            locals.extend(worker_handles.into_iter().map(join_blocking));
            if let Some(late_worker_handles) = late_worker_handles {
                locals.extend(late_worker_handles.lock().drain(..).map(join_blocking));
            }
            if !state.canceled.load(Ordering::Acquire) {
                match state.worker.finish(locals) {
                    Ok(Some(results)) if !results.is_empty() => {
                        let _ = state.result_tx.send(Ok(results));
                    }
                    Ok(_) => {}
                    Err(err) => {
                        let _ = state.result_tx.send(Err(err));
                    }
                }
            }
            drop(state.result_tx);
        })
        .expect("Work coordinator should spawn")
}

fn cancel_closure(
    canceled: Arc<AtomicBool>,
    work_queue_throttle: Arc<WorkQueueThrottle>,
) -> Box<dyn Fn() + Send + Sync> {
    Box::new(move || {
        canceled.store(true, Ordering::Release);
        work_queue_throttle.wake_all();
    })
}

fn scope_buffer<T>(target_items: usize) -> Vec<T> {
    const MAX_INITIAL_CAPACITY: usize = 128;

    Vec::with_capacity(target_items.min(MAX_INITIAL_CAPACITY))
}

struct InlineState<W, Out: Send + 'static, E: Send + 'static> {
    queue: VecDeque<Result<Vec<W>, E>>,
    queued_items: usize,
    canceled: bool,
    output: ItemsWriter<Out, E>,
    output_items: usize,
    output_should_stream: bool,
}

impl<W, Out, E> InlineState<W, Out, E>
where
    Out: Send + 'static,
    E: Send + 'static,
{
    fn new() -> Self {
        Self {
            queue: VecDeque::new(),
            queued_items: 0,
            canceled: false,
            output: ItemsWriter::inline(),
            output_items: 0,
            output_should_stream: false,
        }
    }

    fn submit_input(&mut self, batch: Result<Vec<W>, E>, work_chunk_items: usize) -> bool {
        match batch {
            Ok(batch) => self.submit_work(batch, work_chunk_items),
            Err(err) => self.submit_error(err),
        }
    }

    fn submit_work(&mut self, batch: Vec<W>, work_chunk_items: usize) -> bool {
        if batch.is_empty() {
            return true;
        }
        if self.canceled {
            return false;
        }

        for_each_work_chunk(batch, work_chunk_items, |batch| {
            let items = batch.len();
            self.queued_items += items;
            self.queue.push_back(Ok(batch));
            true
        })
    }

    fn submit_error(&mut self, err: E) -> bool {
        if self.canceled {
            return false;
        }
        self.queued_items += 1;
        self.queue.push_back(Err(err));
        true
    }

    fn send_result_batch(&mut self, batch: Vec<Out>, inline_result_items: usize) -> bool {
        if batch.is_empty() {
            return true;
        }
        if self.canceled {
            return false;
        }

        self.note_output_items(batch.len(), inline_result_items);
        self.output.push_batch(batch)
    }

    fn send_error(&mut self, err: E, inline_result_items: usize) -> bool {
        if self.canceled {
            return false;
        }
        self.note_output_items(1, inline_result_items);
        self.push_error(err)
    }

    fn push_error(&mut self, err: E) -> bool {
        if self.canceled {
            return false;
        }
        self.output.push_error(err)
    }

    fn note_output_items(&mut self, items: usize, inline_result_items: usize) {
        self.output_items += items;
        if self.output_items > inline_result_items {
            self.output_should_stream = true;
        }
    }

    fn cancel_with_error(&mut self, err: E) {
        if !self.canceled {
            let _ = self.push_error(err);
            self.canceled = true;
        }
    }

    fn is_canceled(&self) -> bool {
        self.canceled
    }

    fn should_return_stream(&self) -> bool {
        self.output_should_stream
    }

    fn has_queued_work(&self) -> bool {
        !self.queue.is_empty()
    }

    fn try_pop(&mut self) -> Option<Result<Vec<W>, E>> {
        if self.canceled {
            return None;
        }
        let batch = self.queue.pop_front()?;
        self.queued_items = self
            .queued_items
            .saturating_sub(batch.as_ref().map_or(1, Vec::len));
        Some(batch)
    }

    fn take_output_events(&mut self) -> smallvec::SmallVec<[Result<Vec<Out>, E>; 1]> {
        self.output.take_events()
    }

    fn finish(self) -> Items<Out, E> {
        self.output.finish()
    }
}

struct InlineBackend<'a, W, Out: Send + 'static, E: Send + 'static> {
    inline: &'a mut InlineState<W, Out, E>,
    work_chunk_items: usize,
    inline_result_items: usize,
}

impl<W, Out, E> ScopeBackend<W, Out, E> for InlineBackend<'_, W, Out, E>
where
    Out: Send + 'static,
    E: Send + 'static,
{
    fn submit_work(&mut self, batch: Vec<W>) -> bool {
        self.inline.submit_work(batch, self.work_chunk_items)
    }

    fn submit_error(&mut self, err: E) -> bool {
        self.inline.submit_error(err)
    }

    fn send_result_batch(&mut self, batch: Vec<Out>) -> bool {
        self.inline
            .send_result_batch(batch, self.inline_result_items)
    }

    fn send_error(&mut self, err: E) -> bool {
        self.inline.send_error(err, self.inline_result_items)
    }

    fn is_canceled(&self) -> bool {
        self.inline.is_canceled()
    }

    fn retain_scope_buffers(&self) -> bool {
        false
    }
}

struct ParallelBackend<'a, W: Send + 'static, Out: Send + 'static, E: Send + 'static, K> {
    work_tx: &'a WorkSender<W, E>,
    guard: &'a WorkGuard,
    state: ParallelState<W, Out, E, K>,
    throttle_producer: bool,
    spawn_rest: Option<SpawnRestFn>,
}

impl<W, Out, E, K> ScopeBackend<W, Out, E> for ParallelBackend<'_, W, Out, E, K>
where
    W: Send + 'static,
    Out: Send + 'static,
    E: Send + 'static,
    K: WorkShapeImpl<W, Out, E>,
{
    fn submit_work(&mut self, batch: Vec<W>) -> bool {
        if batch.is_empty() {
            return true;
        }
        if self.is_canceled() {
            return false;
        }
        let mut submitted_chunks = 0usize;
        if !for_each_work_chunk(batch, self.state.work_chunk_items, |batch| {
            let can_submit = if self.throttle_producer {
                self.state
                    .work_queue_throttle
                    .maybe_throttle(&self.state.canceled)
            } else {
                self.state.work_queue_throttle.track_unthrottled();
                true
            };
            if !can_submit {
                return false;
            }

            if self.work_tx.send(batch, self.guard.clone()).is_err() {
                self.state.work_queue_throttle.unthrottle();
                return false;
            }
            submitted_chunks += 1;
            true
        }) {
            return false;
        }
        if submitted_chunks > 0
            && let Some(spawn_rest) = &self.spawn_rest
        {
            spawn_rest(submitted_chunks);
        }
        true
    }

    fn submit_error(&mut self, err: E) -> bool {
        if self.is_canceled() {
            return false;
        }
        let can_submit = if self.throttle_producer {
            self.state
                .work_queue_throttle
                .maybe_throttle(&self.state.canceled)
        } else {
            self.state.work_queue_throttle.track_unthrottled();
            true
        };
        if !can_submit {
            return false;
        }
        if self.work_tx.send_error(err, self.guard.clone()).is_err() {
            self.state.work_queue_throttle.unthrottle();
            return false;
        }
        if let Some(spawn_rest) = &self.spawn_rest {
            spawn_rest(1);
        }
        true
    }

    fn send_result_batch(&mut self, batch: Vec<Out>) -> bool {
        if batch.is_empty() {
            return true;
        }
        if self.is_canceled() {
            return false;
        }
        if self.state.result_tx.send(Ok(batch)).is_err() {
            self.state.cancel();
            return false;
        }
        true
    }

    fn send_error(&mut self, err: E) -> bool {
        if self.is_canceled() {
            return false;
        }
        if self.state.result_tx.send(Err(err)).is_err() {
            self.state.cancel();
            return false;
        }
        true
    }

    fn is_canceled(&self) -> bool {
        self.state.canceled.load(Ordering::Acquire)
    }

    fn retain_scope_buffers(&self) -> bool {
        true
    }
}

struct QueuedWork<W: 'static, E: 'static> {
    batch: Result<Vec<W>, E>,
    guard: WorkGuard,
    work_tx: WorkSender<W, E>,
}

fn for_each_work_chunk<W>(
    work: Vec<W>,
    chunk_items: usize,
    mut f: impl FnMut(Vec<W>) -> bool,
) -> bool {
    if work.len() <= chunk_items {
        return f(work);
    }

    let mut work = work.into_iter();
    loop {
        let chunk = work.by_ref().take(chunk_items).collect::<Vec<_>>();
        if chunk.is_empty() {
            return true;
        }
        if !f(chunk) {
            return false;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::sync::atomic::Ordering;
    use std::thread;
    use std::time::Duration;

    use parking_lot::Mutex;

    use super::*;

    fn unwrap_ready<T, E>(items: Items<T, E>) -> Vec<Vec<T>>
    where
        T: Send + 'static,
        E: Send + 'static,
    {
        match items.into_batches() {
            crate::ItemsBatches::Ready(batches) => batches
                .map(|batch| match batch {
                    Ok(batch) => batch.into_vec(),
                    Err(_) => panic!("expected ready items, got error"),
                })
                .collect(),
            crate::ItemsBatches::Stream(_) => panic!("expected ready items, got stream"),
        }
    }

    fn unwrap_error<T, E>(items: Items<T, E>) -> E
    where
        T: Send + 'static,
        E: Send + 'static,
    {
        match items.into_batches() {
            crate::ItemsBatches::Ready(mut batches) => match batches.next() {
                Some(Err(err)) => err,
                _ => panic!("expected error items, got ready"),
            },
            crate::ItemsBatches::Stream(_) => panic!("expected error items, got stream"),
        }
    }

    fn unwrap_stream<T, E>(items: Items<T, E>) -> Items<T, E>
    where
        T: Send + 'static,
        E: Send + 'static,
    {
        match items.into_batches() {
            crate::ItemsBatches::Ready(_) => panic!("expected stream items, got ready"),
            crate::ItemsBatches::Stream(stream) => Items::Stream(stream),
        }
    }

    #[test]
    fn worker_budget_claims_only_available_capacity() {
        let spawner = crate::LimitedSpawner::new(4);

        assert_eq!(spawner.claim(10), 4);
        assert_eq!(spawner.available.load(Ordering::Acquire), 0);

        assert_eq!(spawner.claim(10), 0);
        assert_eq!(spawner.available.load(Ordering::Acquire), 0);

        spawner.release(1);
        assert_eq!(spawner.claim(10), 1);
        assert_eq!(spawner.available.load(Ordering::Acquire), 0);

        spawner.release(4);
        assert_eq!(spawner.available.load(Ordering::Acquire), 4);
    }

    #[test]
    fn worker_soft_limit_is_large_enough_for_io_but_bounded_for_edenfs() {
        let limit = worker_soft_limit();

        assert!((MIN_WORKER_SOFT_LIMIT..=MAX_WORKER_SOFT_LIMIT).contains(&limit));
    }

    #[test]
    fn stays_inline_below_threshold_including_generated_work() {
        let caller = thread::current().id();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_worker = Arc::clone(&seen);
        Work::run::<_, (), _, _>(
            WorkOptions::new().inline_items(10),
            Items::ready(vec![0usize]),
            WorkShape::batch(move |batch, scope| -> Result<(), &'static str> {
                let batch = batch?;
                seen_worker.lock().push(thread::current().id());
                if batch[0] < 3 {
                    scope.submit_work(batch[0] + 1);
                }
                Ok(())
            }),
        )
        .drain_until_error()
        .unwrap();

        assert!(seen.lock().iter().all(|id| *id == caller));
    }

    #[test]
    fn promotes_initial_fanout_to_executor_workers() {
        let caller = thread::current().id();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_worker = Arc::clone(&seen);
        Work::run::<_, (), _, _>(
            WorkOptions::new().inline_items(1).max_workers(4),
            Items::ready((0..16).collect::<Vec<_>>()),
            WorkShape::batch(move |_batch, _scope| -> Result<(), &'static str> {
                seen_worker.lock().push(thread::current().id());
                Ok(())
            }),
        )
        .drain_until_error()
        .unwrap();

        assert!(seen.lock().iter().any(|id| *id != caller));
    }

    #[test]
    fn stays_inline_until_work_spans_two_chunks() {
        let caller = thread::current().id();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_worker = Arc::clone(&seen);
        Work::run::<_, (), _, _>(
            WorkOptions::new()
                .inline_items(4)
                .work_chunk_items(4)
                .max_workers(4),
            Items::ready((0..7).collect::<Vec<_>>()),
            WorkShape::batch(move |_batch, _scope| -> Result<(), &'static str> {
                seen_worker.lock().push(thread::current().id());
                Ok(())
            }),
        )
        .drain_until_error()
        .unwrap();

        assert!(seen.lock().iter().all(|id| *id == caller));
    }

    #[test]
    fn single_large_batch_stays_inline_when_chunking_keeps_it_whole() {
        let caller = thread::current().id();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_worker = Arc::clone(&seen);
        Work::run::<_, (), _, _>(
            WorkOptions::new()
                .inline_items(100)
                .work_chunk_items(100)
                .max_workers(4),
            Items::ready((0..16).collect::<Vec<_>>()),
            WorkShape::batch(move |_batch, _scope| -> Result<(), &'static str> {
                seen_worker.lock().push(thread::current().id());
                Ok(())
            }),
        )
        .drain_until_error()
        .unwrap();

        assert!(seen.lock().iter().all(|id| *id == caller));
    }

    #[test]
    fn chunked_large_batch_promotes_to_executor_workers() {
        let caller = thread::current().id();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_worker = Arc::clone(&seen);
        Work::run::<_, (), _, _>(
            WorkOptions::new()
                .inline_items(4)
                .work_chunk_items(4)
                .max_workers(4),
            Items::ready((0..16).collect::<Vec<_>>()),
            WorkShape::batch(move |_batch, _scope| -> Result<(), &'static str> {
                seen_worker.lock().push(thread::current().id());
                Ok(())
            }),
        )
        .drain_until_error()
        .unwrap();

        assert!(seen.lock().iter().any(|id| *id != caller));
    }

    #[test]
    fn streaming_input_feeds_executor_workers() {
        let caller = thread::current().id();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_worker = Arc::clone(&seen);
        let input: Items<usize, &'static str> =
            Items::stream(vec![Ok(vec![0usize, 1, 2, 3]), Ok(vec![4, 5, 6, 7])].into_iter());
        let mut result = Work::run(
            WorkOptions::new()
                .inline_items(2)
                .work_chunk_items(2)
                .max_workers(4),
            input,
            WorkShape::batch(move |batch, scope| {
                let batch = batch?;
                seen_worker.lock().push(thread::current().id());
                scope.send_result(batch.into_iter().map(|item| item * 2));
                Ok(())
            }),
        )
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

        result.sort();
        assert_eq!(result, vec![0, 2, 4, 6, 8, 10, 12, 14]);
        assert!(seen.lock().iter().any(|id| *id != caller));
    }

    #[test]
    fn workset_output_feeds_next_workset() {
        let first = Work::try_map(
            WorkOptions::new().inline_items(1).max_workers(4),
            Items::ready((0..8usize).collect::<Vec<_>>()),
            |item| -> Result<usize, &'static str> { Ok(item + 1) },
        );

        let mut second = Work::run(
            WorkOptions::new().inline_items(1).max_workers(4),
            first,
            WorkShape::batch(move |batch, scope| -> Result<(), &'static str> {
                let batch = batch?;
                scope.send_result(batch.into_iter().map(|item| item * 2));
                Ok(())
            }),
        )
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

        second.sort();
        assert_eq!(second, vec![2, 4, 6, 8, 10, 12, 14, 16]);
    }

    #[test]
    fn input_error_from_previous_workset_propagates() {
        let first = Work::try_map(
            WorkOptions::new().inline_items(10),
            Items::ready(vec![1usize, 2, 3]),
            |item| if item == 2 { Err("bad") } else { Ok(item) },
        );

        let result = Work::run(
            WorkOptions::new().inline_items(10),
            first,
            WorkShape::batch(move |batch, scope| -> Result<(), &'static str> {
                let batch = batch?;
                scope.send_result(batch);
                Ok(())
            }),
        )
        .drain_until_error();

        assert_eq!(result, Err("bad"));
    }

    #[test]
    fn batch_worker_can_emit_nonfatal_errors() {
        let items = Work::run(
            WorkOptions::new().inline_items(10),
            Items::ready(vec![1usize, 2, 3]),
            WorkShape::batch(move |batch, scope| -> Result<(), &'static str> {
                let batch = batch?;
                for item in batch {
                    if item == 2 {
                        scope.send_error("bad");
                    } else {
                        scope.send_result([item]);
                    }
                }
                Ok(())
            }),
        );

        let events = items.into_batches().collect::<Vec<_>>();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].as_ref().unwrap().as_slice(), &[1]);
        assert_eq!(events[1], Err("bad"));
        assert_eq!(events[2].as_ref().unwrap().as_slice(), &[3]);
    }

    #[test]
    fn inline_error_threshold_promotes_when_work_remains() {
        let items = Work::run(
            WorkOptions::new()
                .inline_items(100)
                .inline_result_items(1)
                .max_workers(2),
            Items::ready(vec![0usize]),
            WorkShape::batch(move |batch, scope| -> Result<(), &'static str> {
                let batch = batch?;
                for item in batch {
                    if item == 0 {
                        scope.send_error("first");
                        scope.send_error("second");
                        scope.submit_work(1);
                    } else {
                        scope.send_result([item]);
                    }
                }
                Ok(())
            }),
        );

        let crate::ItemsBatches::Stream(stream) = items.into_batches() else {
            panic!("error output above inline threshold should promote while work remains");
        };
        let events = stream.collect::<Vec<_>>();

        assert_eq!(events[0], Err("first"));
        assert_eq!(events[1], Err("second"));
        assert_eq!(events[2].as_ref().unwrap().as_slice(), &[1]);
    }

    #[test]
    fn promoted_batch_worker_can_emit_nonfatal_errors() {
        let caller = thread::current().id();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_worker = Arc::clone(&seen);
        let items = Work::run(
            WorkOptions::new().inline_items(1).max_workers(2),
            Items::ready((0..8usize).collect::<Vec<_>>()),
            WorkShape::batch(move |batch, scope| -> Result<(), &'static str> {
                let batch = batch?;
                seen_worker.lock().push(thread::current().id());
                for item in batch {
                    if item == 2 {
                        scope.send_error("bad");
                    } else {
                        scope.send_result([item]);
                    }
                }
                Ok(())
            }),
        );

        let mut values = Vec::new();
        let mut errors = Vec::new();
        for event in items.into_batches() {
            match event {
                Ok(batch) => values.extend(batch),
                Err(err) => errors.push(err),
            }
        }

        values.sort_unstable();
        assert_eq!(values, vec![0, 1, 3, 4, 5, 6, 7]);
        assert_eq!(errors, vec!["bad"]);
        assert!(seen.lock().iter().any(|id| *id != caller));
    }

    #[test]
    fn promotes_generated_fanout_to_executor_workers() {
        let caller = thread::current().id();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_worker = Arc::clone(&seen);
        Work::run::<_, (), _, _>(
            WorkOptions::new().inline_items(4).max_workers(4),
            Items::ready(vec![0usize]),
            WorkShape::batch(move |batch, scope| -> Result<(), &'static str> {
                let batch = batch?;
                seen_worker.lock().push(thread::current().id());
                if batch[0] == 0 {
                    (1..16).for_each(|item| {
                        scope.submit_work(item);
                    });
                }
                Ok(())
            }),
        )
        .drain_until_error()
        .unwrap();

        let ids = seen.lock().iter().copied().collect::<HashSet<_>>();
        assert!(ids.iter().any(|id| *id != caller));
    }

    #[test]
    fn first_error_cancels_pending_work() {
        let processed = Arc::new(Mutex::new(0usize));
        let processed_worker = Arc::clone(&processed);
        let result = Work::run::<_, (), _, _>(
            WorkOptions::new().inline_items(1).max_workers(2),
            Items::ready((0..32).collect::<Vec<_>>()),
            WorkShape::batch(move |_batch, _scope| -> Result<(), &'static str> {
                *processed_worker.lock() += 1;
                Err("boom")
            }),
        )
        .drain_until_error();

        assert_eq!(result, Err("boom"));
        assert!(*processed.lock() < 32);
    }

    #[test]
    fn single_ready_error_is_processed_by_batch_worker() {
        let result = Work::run(
            WorkOptions::new(),
            Items::<usize, &'static str>::error("input"),
            WorkShape::batch(|batch, scope| -> Result<(), &'static str> {
                match batch {
                    Ok(_) => panic!("expected input error"),
                    Err(err) => {
                        scope.send_result([err.len()]);
                        Ok(())
                    }
                }
            }),
        )
        .into_iter()
        .collect::<Result<Vec<_>, _>>();

        assert_eq!(result, Ok(vec![5]));
    }

    #[test]
    fn streaming_input_error_flushes_pending_work_first() {
        let input: Items<usize, &'static str> =
            Items::stream([Ok(vec![1]), Err("bad")].into_iter());
        let result = Work::run::<usize, &'static str, &'static str, _>(
            WorkOptions::new()
                .inline_items(1)
                .work_chunk_items(16)
                .max_workers(1),
            input,
            WorkShape::batch(
                |batch: Result<Vec<usize>, &'static str>,
                 scope: &mut WorkScope<'_, usize, &'static str, &'static str>|
                 -> Result<(), &'static str> {
                    match batch {
                        Ok(batch) => {
                            scope.send_result(batch.into_iter().map(|_| "work"));
                        }
                        Err(err) => {
                            assert_eq!(err, "bad");
                            scope.send_result(["error"]);
                        }
                    }
                    Ok(())
                },
            ),
        )
        .into_iter()
        .collect::<Result<Vec<_>, _>>();

        assert_eq!(result, Ok(vec!["work", "error"]));
    }

    #[test]
    fn pre_promotion_errors_do_not_block_on_result_queue() {
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let result = Work::run::<_, (), _, _>(
                WorkOptions::new()
                    .inline_items(4)
                    .result_queue_size(1)
                    .result_batch_items(1),
                Items::ready(vec![0usize]),
                WorkShape::batch(|batch, scope| -> Result<(), usize> {
                    let batch = batch?;
                    for item in batch {
                        if item == 0 {
                            for err in 0..4 {
                                scope.send_error(err);
                            }
                            scope.submit_work((1..32).collect::<Vec<_>>());
                        }
                    }
                    Ok(())
                }),
            )
            .drain_until_error();
            tx.send(result).unwrap();
        });

        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)),
            Ok(Err(0)),
            "Work::run blocked before returning a drainable result stream"
        );
    }

    #[test]
    fn try_map_returns_first_error() {
        let result = Work::try_map(
            WorkOptions::new().inline_items(1),
            Items::ready(vec![
                Box::new(|| Ok(())) as Box<dyn FnOnce() -> Result<(), &'static str> + Send>,
                Box::new(|| Err("boom")) as Box<dyn FnOnce() -> Result<(), &'static str> + Send>,
            ]),
            |work| work(),
        )
        .drain_until_error();

        assert_eq!(result, Err("boom"));
    }

    #[test]
    fn inline_results_stay_ready() {
        let result = Work::run(
            WorkOptions::new().inline_items(10),
            Items::ready(vec![1usize, 2]),
            WorkShape::batch(move |batch, scope| -> Result<(), &'static str> {
                let batch = batch?;
                scope.send_result(batch.into_iter().map(|item| item * 2).collect::<Vec<_>>());
                Ok(())
            }),
        );

        assert_eq!(
            unwrap_ready(result)
                .into_iter()
                .flatten()
                .collect::<Vec<_>>(),
            vec![2, 4]
        );
    }

    #[test]
    fn flat_worker_uses_inline_fast_path() {
        let result: Items<usize> = Work::map(
            WorkOptions::new().inline_items(10),
            Items::ready(vec![1usize, 2]),
            |item| item * 2,
        );

        assert_eq!(
            unwrap_ready(result)
                .into_iter()
                .flatten()
                .collect::<Vec<_>>(),
            vec![2, 4]
        );
    }

    #[test]
    fn try_flat_worker_returns_first_error() {
        let result = Work::try_map(
            WorkOptions::new().inline_items(10),
            Items::ready(vec![1usize, 2, 3]),
            |item| if item == 2 { Err("bad") } else { Ok(item * 2) },
        );

        assert_eq!(unwrap_error(result), "bad");
    }

    #[test]
    fn output_threshold_stays_ready_when_work_is_finished() {
        let result = Work::run(
            WorkOptions::new()
                .inline_items(10)
                .inline_result_items(2)
                .result_queue_size(2),
            Items::ready(vec![1usize, 2, 3]),
            WorkShape::batch(move |batch, scope| -> Result<(), &'static str> {
                let batch = batch?;
                scope.send_result(batch);
                Ok(())
            }),
        );

        assert_eq!(
            unwrap_ready(result)
                .into_iter()
                .flatten()
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn output_threshold_promotes_to_stream_when_work_remains() {
        let result = Work::run(
            WorkOptions::new()
                .inline_items(10)
                .inline_result_items(2)
                .result_queue_size(2),
            Items::ready(vec![1usize]),
            WorkShape::batch(move |batch, scope| -> Result<(), &'static str> {
                let batch = batch?;
                for item in batch {
                    scope.send_result([item, item + 1, item + 2]);
                    if item == 1 {
                        scope.submit_work(4);
                    }
                }
                Ok(())
            }),
        );

        let result = unwrap_stream(result);
        let items = result.into_iter().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(items, vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn work_queue_throttle_does_not_underflow() {
        let throttle = WorkQueueThrottle::new(1);

        let canceled = AtomicBool::new(false);
        assert!(throttle.maybe_throttle(&canceled));
        assert_eq!(throttle.queued.load(Ordering::Acquire), 1);

        throttle.unthrottle();
        assert_eq!(throttle.queued.load(Ordering::Acquire), 0);
    }

    #[test]
    fn batch_finalize_collects_worker_local_state() {
        let mut result = Work::run(
            WorkOptions::new()
                .inline_items(1)
                .work_chunk_items(1)
                .max_workers(4),
            Items::ready((0..16).collect::<Vec<_>>()),
            WorkShape::batch_finalize(
                Vec::<usize>::new,
                |batch: Result<Vec<usize>, &'static str>,
                 scope: &mut WorkScope<'_, usize, usize, &'static str, Vec<usize>>| {
                    let batch = batch?;
                    scope.local_mut().extend(batch.iter().copied());
                    scope.send_result(batch);
                    Ok(())
                },
                |locals| {
                    Ok(Some(vec![
                        locals.into_iter().map(|local| local.len()).sum(),
                    ]))
                },
            ),
        )
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

        result.sort();
        assert_eq!(result, (0..=16).collect::<Vec<_>>());
    }
}
