/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Lightweight ready-or-stream transport for batched synchronous APIs.
//!
//! `Items<T>` has no executor policy. It only models whether a producer has `T` results ready now
//! or results that should be pulled from an iterator later. This keeps signature-only users, such
//! as store traits, from depending on thread pools, channels, or async runtimes.
//!
//! `Items` transports small-vector batches internally, so callers can avoid per-item channel and
//! allocation overhead. Use item-by-item iteration only as a compatibility fallback.
//!
//! Construct ready values when the producer already has results in hand:
//!
//! ```rust
//! # use slex_items::Items;
//! let items: Items<i32> = Items::ready(vec![1, 2, 3]);
//! ```
//!
//! Construct streams when results are produced over time. Stream items are fallible batches:
//!
//! ```rust
//! # use slex_items::Items;
//! let items: Items<i32, &'static str> = Items::stream([Ok(vec![1, 2]), Ok(vec![3])].into_iter());
//! ```
//!
//! Adapt item-by-item producers only when the source cannot produce batches:
//!
//! ```rust
//! # use slex_items::Items;
//! let items: Items<i32, &'static str> = Items::item_stream([Ok(1), Ok(2)].into_iter());
//! ```
//!
//! Forward results with [`Items::into_batches`] to preserve batching:
//!
//! ```rust
//! # use slex_items::Items;
//! let items: Items<i32> = Items::ready(vec![1, 2, 3]);
//! let batches = items.into_batches().collect::<Result<Vec<_>, _>>().unwrap();
//! assert_eq!(batches[0].as_slice(), &[1, 2, 3]);
//! ```
//!
//! Combine transports with [`Items::chain`] without flattening ready batches:
//!
//! ```rust
//! # use slex_items::Items;
//! let items: Items<i32> = Items::ready(vec![1, 2]).chain(Items::ready([3]));
//! let values = items.into_iter().collect::<Result<Vec<_>, _>>().unwrap();
//! assert_eq!(values, vec![1, 2, 3]);
//! ```

use std::convert::Infallible;

use smallvec::SmallVec;

/// One batch of items.
///
/// The inline capacity is one because the common compatibility path produces one item at a time,
/// while larger batches can spill to the heap and preserve their existing `Vec` allocation.
pub type Batch<T> = SmallVec<[T; 1]>;

/// Default item count for input batches passed between `Items` producers and consumers.
pub const DEFAULT_INPUT_BATCH_SIZE: usize = 16;

/// Lifetime-scoped ready-or-stream transport for batched APIs.
///
/// Ready mode avoids channel and worker overhead for small/local work. Stream mode yields batches
/// as they become available. Prefer consuming batches with [`Items::into_batches`] when forwarding
/// results to another batching API. Item-by-item iteration exists mostly for compatibility.
pub enum ScopedItems<'a, T, E = Infallible> {
    /// All fallible batches are already available.
    ///
    /// `SmallVec` keeps the common single-batch ready path allocation-free while still allowing
    /// `chain()` to combine multiple ready producers without degrading to a boxed stream. Errors
    /// are stored alongside ready batches, just like stream errors, so the consumer decides whether
    /// an error should stop iteration.
    Ready(SmallVec<[Result<Batch<T>, E>; 1]>),
    /// A producer-backed stream of fallible batches.
    ///
    /// This is used when results are generated over time, often by background work or remote I/O,
    /// and preserves batching across pipeline stages.
    Stream(Box<dyn Iterator<Item = Result<Batch<T>, E>> + Send + 'a>),
    /// A producer-backed stream of fallible individual items.
    ///
    /// Item iteration stays item-at-a-time, while [`Items::into_batches`] coalesces adjacent
    /// successes for batch-oriented consumers like `slex::Work`.
    ItemStream(Box<dyn Iterator<Item = Result<T, E>> + Send + 'a>),
}

/// Ready-or-stream transport for owned/static batched APIs.
pub type Items<T, E = Infallible> = ScopedItems<'static, T, E>;

impl<'a, T, E> ScopedItems<'a, T, E> {
    /// Construct an empty ready result with no stream/channel allocation.
    pub fn empty() -> Self {
        Self::Ready(SmallVec::new())
    }

    /// Construct one ready success batch with no stream/channel allocation.
    ///
    /// Use [`Items::empty`] for an empty result. `ready(Vec::new())` means the producer has emitted
    /// one empty batch, which can be observable to batch consumers.
    pub fn ready(items: impl Into<Batch<T>>) -> Self {
        Self::Ready([Ok(items.into())].into())
    }

    /// Construct a ready error without creating a stream.
    pub fn error(err: E) -> Self {
        Self::Ready([Err(err)].into())
    }

    /// Construct a stream of fallible batches.
    pub fn stream(iter: impl Iterator<Item = Result<Vec<T>, E>> + Send + 'a) -> Self
    where
        T: 'a,
        E: 'a,
    {
        Self::Stream(Box::new(iter.map(|batch| batch.map(Into::into))))
    }

    /// Construct a stream from a fallible item-at-a-time source.
    ///
    /// The adapter batches adjacent successful items so legacy item iterators do not force
    /// downstream pipelines to process one-item batches. New code should still prefer
    /// [`Items::ready`] or [`Items::stream`] when the producer already has batches.
    pub fn item_stream(iter: impl Iterator<Item = Result<T, E>> + Send + 'a) -> Self
    where
        T: Send + 'a,
        E: Send + 'a,
    {
        Self::ItemStream(Box::new(iter))
    }

    /// Consume this value as fallible batches.
    ///
    /// This is the efficient forwarding path.
    pub fn into_batches(self) -> ItemsBatches<'a, T, E>
    where
        T: Send + 'a,
        E: Send + 'a,
    {
        match self {
            Self::Ready(items) => ItemsBatches::Ready(items.into_iter()),
            Self::Stream(iter) => ItemsBatches::Stream(iter),
            Self::ItemStream(iter) => ItemsBatches::Stream(Box::new(ItemStreamBatcher::new(iter))),
        }
    }

    /// Map each fallible batch, preserving ready-vs-stream shape.
    ///
    /// This is the batch-level equivalent of `Iterator::map` for fallible transforms. Unlike
    /// `Items::stream(items.into_batches().map(...))`, ready inputs remain ready and do not pay for
    /// a boxed stream.
    pub fn map_batch<U, E2, B, F>(self, mut f: F) -> ScopedItems<'a, U, E2>
    where
        T: Send + 'a,
        U: Send + 'a,
        E: Send + 'a,
        E2: Send + 'a,
        B: Into<Batch<U>>,
        F: FnMut(Result<Batch<T>, E>) -> Result<B, E2> + Send + 'a,
    {
        match self.into_batches() {
            ItemsBatches::Ready(batches) => ScopedItems::Ready(
                batches
                    .into_iter()
                    .map(|batch| f(batch).map(Into::into))
                    .collect(),
            ),
            ItemsBatches::Stream(iter) => {
                ScopedItems::Stream(Box::new(iter.map(move |batch| f(batch).map(Into::into))))
            }
        }
    }

    /// Map each item, preserving ready-vs-stream shape and batch success fast paths.
    ///
    /// Successful items stay grouped with their input batch. If a mapped item fails, successful
    /// items before the error are emitted as one batch, then the error is emitted, then later
    /// successful items can form another batch.
    pub fn try_map_item<U, E2, F>(self, mut f: F) -> ScopedItems<'a, U, E2>
    where
        T: Send + 'a,
        U: Send + 'a,
        E: Into<E2> + Send + 'a,
        E2: Send + 'a,
        F: FnMut(T) -> Result<U, E2> + Send + 'a,
    {
        self.flat_map_batch(move |batch| try_map_item_batch(batch, &mut f))
    }

    /// Map each input batch to zero or more output batches or errors.
    ///
    /// This is useful for adapters that receive a batch containing mixed successes and failures
    /// and want to keep successful values batched while emitting each failure separately.
    pub fn flat_map_batch<U, E2, F, I>(self, mut f: F) -> ScopedItems<'a, U, E2>
    where
        T: Send + 'a,
        U: Send + 'a,
        E: Into<E2> + Send + 'a,
        E2: Send + 'a,
        F: FnMut(Batch<T>) -> I + Send + 'a,
        I: IntoIterator<Item = Result<Vec<U>, E2>> + 'a,
        I::IntoIter: Send,
    {
        match self {
            Self::Ready(batches) => {
                let mut mapped = SmallVec::new();
                for batch in batches {
                    match batch {
                        Ok(batch) => {
                            mapped.extend(f(batch).into_iter().map(|batch| batch.map(Into::into)))
                        }
                        Err(err) => mapped.push(Err(err.into())),
                    }
                }
                ScopedItems::Ready(mapped)
            }
            Self::Stream(iter) => ScopedItems::Stream(Box::new(FlatMapBatchStream::new(iter, f))),
            Self::ItemStream(iter) => ScopedItems::Stream(Box::new(FlatMapBatchStream::new(
                Box::new(ItemStreamBatcher::new(iter)),
                f,
            ))),
        }
    }

    /// Concatenate two item transports without flattening batches.
    ///
    /// Ready-ready chaining keeps all ready batches inline using a small-vector representation.
    /// Mixed ready/stream chains become a lazy stream.
    pub fn chain(self, other: Self) -> Self
    where
        T: Send + 'a,
        E: Send + 'a,
    {
        match (self, other) {
            (Self::Ready(mut left), Self::Ready(right)) => {
                left.extend(right);
                Self::Ready(left)
            }
            (Self::Ready(left), right) => {
                Self::Stream(Box::new(left.into_iter().chain(right.into_batches())))
            }
            (left, Self::Ready(right)) => Self::Stream(Box::new(left.into_batches().chain(right))),
            (left, right) => {
                Self::Stream(Box::new(left.into_batches().chain(right.into_batches())))
            }
        }
    }

    /// Drain all output and return the first error encountered.
    ///
    /// Unlike [`Items::drain_until_error`], this keeps pulling after an error so producers finish
    /// naturally. Use this when cancellation is not desired but the caller still wants to know
    /// whether any error occurred.
    pub fn drain(self) -> Result<(), E>
    where
        T: Send + 'a,
        E: Send + 'a,
    {
        let mut first_error = None;
        for batch in self.into_batches() {
            if let Err(err) = batch
                && first_error.is_none()
            {
                first_error = Some(err);
            }
        }
        match first_error {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    /// Drain output until the first error.
    ///
    /// Returning early drops the remaining stream. For `Work` streams that cancels unfinished work.
    /// Use this for fail-fast operations where the first error makes remaining output irrelevant.
    pub fn drain_until_error(self) -> Result<(), E>
    where
        T: Send + 'a,
        E: Send + 'a,
    {
        for batch in self.into_batches() {
            batch?;
        }
        Ok(())
    }
}

struct ItemStreamBatcher<I, T, E> {
    iter: I,
    batch: Vec<T>,
    pending_error: Option<E>,
}

impl<I, T, E> ItemStreamBatcher<I, T, E> {
    fn new(iter: I) -> Self {
        Self {
            iter,
            batch: Vec::with_capacity(DEFAULT_INPUT_BATCH_SIZE),
            pending_error: None,
        }
    }

    fn take_batch(&mut self) -> Batch<T> {
        std::mem::replace(
            &mut self.batch,
            Vec::with_capacity(DEFAULT_INPUT_BATCH_SIZE),
        )
        .into()
    }
}

impl<I, T, E> Iterator for ItemStreamBatcher<I, T, E>
where
    I: Iterator<Item = Result<T, E>>,
{
    type Item = Result<Batch<T>, E>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(err) = self.pending_error.take() {
            return Some(Err(err));
        }

        for item in self.iter.by_ref() {
            match item {
                Ok(item) => {
                    self.batch.push(item);
                    if self.batch.len() >= DEFAULT_INPUT_BATCH_SIZE {
                        return Some(Ok(self.take_batch()));
                    }
                }
                Err(err) => {
                    if self.batch.is_empty() {
                        return Some(Err(err));
                    }
                    self.pending_error = Some(err);
                    return Some(Ok(self.take_batch()));
                }
            }
        }

        if self.batch.is_empty() {
            None
        } else {
            Some(Ok(self.take_batch()))
        }
    }
}

fn try_map_item_batch<T, U, E, F>(batch: Batch<T>, f: &mut F) -> SmallVec<[Result<Vec<U>, E>; 1]>
where
    F: FnMut(T) -> Result<U, E>,
{
    let mut events = SmallVec::new();
    let mut success = Vec::with_capacity(batch.len());
    for item in batch {
        match f(item) {
            Ok(item) => success.push(item),
            Err(err) => {
                if !success.is_empty() {
                    events.push(Ok(std::mem::take(&mut success)));
                }
                events.push(Err(err));
            }
        }
    }
    if !success.is_empty() || events.is_empty() {
        events.push(Ok(success));
    }
    events
}

/// Lowered batch iterator from [`Items`].
///
/// `Ready` preserves the no-stream fast path. `Stream` covers producer-backed batch streams.
pub enum ItemsBatches<'a, T, E = Infallible> {
    Ready(smallvec::IntoIter<[Result<Batch<T>, E>; 1]>),
    Stream(Box<dyn Iterator<Item = Result<Batch<T>, E>> + Send + 'a>),
}

impl<T, E> Iterator for ItemsBatches<'_, T, E> {
    type Item = Result<Batch<T>, E>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Ready(items) => items.next(),
            Self::Stream(iter) => iter.next(),
        }
    }
}

struct FlatMapBatchStream<'a, T, E, U, E2, F, I>
where
    I: IntoIterator<Item = Result<Vec<U>, E2>>,
{
    iter: Box<dyn Iterator<Item = Result<Batch<T>, E>> + Send + 'a>,
    f: F,
    current: Option<I::IntoIter>,
}

impl<'a, T, E, U, E2, F, I> FlatMapBatchStream<'a, T, E, U, E2, F, I>
where
    I: IntoIterator<Item = Result<Vec<U>, E2>>,
{
    fn new(iter: Box<dyn Iterator<Item = Result<Batch<T>, E>> + Send + 'a>, f: F) -> Self {
        Self {
            iter,
            f,
            current: None,
        }
    }
}

impl<T, E, U, E2, F, I> Iterator for FlatMapBatchStream<'_, T, E, U, E2, F, I>
where
    E: Into<E2>,
    F: FnMut(Batch<T>) -> I,
    I: IntoIterator<Item = Result<Vec<U>, E2>>,
{
    type Item = Result<Batch<U>, E2>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(current) = &mut self.current
                && let Some(item) = current.next()
            {
                return Some(item.map(Into::into));
            }
            self.current = None;

            match self.iter.next()? {
                Ok(batch) => {
                    self.current = Some((self.f)(batch).into_iter());
                }
                Err(err) => return Some(Err(err.into())),
            }
        }
    }
}

/// Compatibility iterator that flattens [`Items`] into individual fallible items.
pub struct ItemsIntoIter<'a, T, E = Infallible> {
    kind: ItemsIntoIterKind<'a, T, E>,
}

enum ItemsIntoIterKind<'a, T, E = Infallible> {
    Batches {
        batches: ItemsBatches<'a, T, E>,
        current: Option<smallvec::IntoIter<[T; 1]>>,
    },
    Items(Box<dyn Iterator<Item = Result<T, E>> + Send + 'a>),
}

impl<T, E> Iterator for ItemsIntoIter<'_, T, E> {
    type Item = Result<T, E>;

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.kind {
            ItemsIntoIterKind::Items(iter) => iter.next(),
            ItemsIntoIterKind::Batches { batches, current } => loop {
                if let Some(items) = current {
                    if let Some(item) = items.next() {
                        return Some(Ok(item));
                    }
                    *current = None;
                }

                match batches.next()? {
                    Ok(batch) => {
                        *current = Some(batch.into_iter());
                    }
                    Err(err) => return Some(Err(err)),
                }
            },
        }
    }
}

impl<'a, T, E> IntoIterator for ScopedItems<'a, T, E>
where
    T: Send + 'a,
    E: Send + 'a,
{
    type IntoIter = ItemsIntoIter<'a, T, E>;
    type Item = Result<T, E>;

    fn into_iter(self) -> Self::IntoIter {
        let kind = match self {
            Self::ItemStream(iter) => ItemsIntoIterKind::Items(iter),
            items => ItemsIntoIterKind::Batches {
                batches: items.into_batches(),
                current: None,
            },
        };
        ItemsIntoIter { kind }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    use super::*;

    #[test]
    fn empty_is_ready_with_no_batches() {
        let items: Items<i32> = Items::empty();

        assert!(matches!(items.into_batches(), ItemsBatches::Ready(_)));
    }

    #[test]
    fn empty_has_no_batches() {
        let items: Items<i32> = Items::empty();
        let mut batches = items.into_batches();

        assert!(batches.next().is_none());
    }

    #[test]
    fn chain_ready_batches_without_streaming() {
        let items: Items<i32> = Items::ready(vec![1, 2]).chain(Items::ready(vec![3, 4]));

        assert!(matches!(items.into_batches(), ItemsBatches::Ready(_)));
    }

    #[test]
    fn chain_ready_before_stream() {
        let items: Items<i32> =
            Items::ready(vec![1, 2]).chain(Items::stream([Ok(vec![3]), Ok(vec![4])].into_iter()));
        assert_eq!(
            items.into_iter().collect::<Result<Vec<_>, _>>().unwrap(),
            vec![1, 2, 3, 4]
        );
    }

    #[test]
    fn item_stream_batches_successes() {
        let items: Items<usize, &'static str> =
            Items::item_stream((0..DEFAULT_INPUT_BATCH_SIZE + 2).map(Ok::<_, &'static str>));
        let batches = items.into_batches().collect::<Result<Vec<_>, _>>().unwrap();

        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].len(), DEFAULT_INPUT_BATCH_SIZE);
        assert_eq!(
            batches[1].as_slice(),
            &[DEFAULT_INPUT_BATCH_SIZE, DEFAULT_INPUT_BATCH_SIZE + 1]
        );
    }

    #[test]
    fn item_stream_into_iter_stays_item_lazy() {
        let pulled = Arc::new(AtomicUsize::new(0));
        let pulled_in_iter = Arc::clone(&pulled);
        let items: Items<usize, &'static str> = Items::item_stream((0..130).map(move |item| {
            pulled_in_iter.fetch_add(1, Ordering::SeqCst);
            Ok(item)
        }));
        let mut iter = items.into_iter();

        assert_eq!(pulled.load(Ordering::SeqCst), 0);
        assert_eq!(iter.next(), Some(Ok(0)));
        assert_eq!(pulled.load(Ordering::SeqCst), 1);
        assert_eq!(iter.next(), Some(Ok(1)));
        assert_eq!(pulled.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn item_stream_flushes_before_error() {
        let items: Items<usize, &'static str> =
            Items::item_stream([Ok(1), Ok(2), Err("bad"), Ok(3)].into_iter());
        let batches = items.into_batches().collect::<Vec<_>>();

        assert_eq!(batches.len(), 3);
        assert_eq!(batches[0].as_ref().unwrap().as_slice(), &[1, 2]);
        assert_eq!(batches[1], Err("bad"));
        assert_eq!(batches[2].as_ref().unwrap().as_slice(), &[3]);
    }

    #[test]
    fn map_batch_preserves_ready() {
        let items: Items<i32, Infallible> = Items::ready(vec![1, 2]);
        let mapped: Items<i32, Infallible> = items
            .map_batch(|batch| Ok(batch?.into_iter().map(|item| item * 2).collect::<Vec<_>>()));

        assert!(matches!(mapped.into_batches(), ItemsBatches::Ready(_)));
    }

    #[test]
    fn map_batch_maps_stream_lazily() {
        let mapped_batches = Arc::new(AtomicUsize::new(0));
        let mapped_batches_in_map = Arc::clone(&mapped_batches);
        let items: Items<i32, Infallible> =
            Items::stream([Ok(vec![1, 2]), Ok(vec![3])].into_iter());
        let mapped: Items<i32, Infallible> = items.map_batch(move |batch| {
            mapped_batches_in_map.fetch_add(1, Ordering::SeqCst);
            Ok(batch?.into_iter().map(|item| item * 2).collect::<Vec<_>>())
        });

        assert_eq!(mapped_batches.load(Ordering::SeqCst), 0);
        let mut batches = mapped.into_batches();
        assert_eq!(mapped_batches.load(Ordering::SeqCst), 0);
        assert_eq!(batches.next().unwrap().unwrap().as_slice(), &[2, 4]);
        assert_eq!(mapped_batches.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn try_map_item_preserves_success_batch() {
        let items: Items<i32, &'static str> = Items::ready(vec![1, 2, 3]);
        let mapped: Items<i32, &'static str> = items.try_map_item(|item| Ok(item * 2));
        let batches = mapped
            .into_batches()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].as_slice(), &[2, 4, 6]);
    }

    #[test]
    fn try_map_item_preserves_empty_batch() {
        let items: Items<i32, &'static str> = Items::ready(vec![]);
        let mapped: Items<i32, &'static str> = items.try_map_item(|item| Ok(item * 2));
        let batches = mapped
            .into_batches()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(batches.len(), 1);
        assert!(batches[0].is_empty());
    }

    #[test]
    fn try_map_item_splits_errors_from_successes() {
        let items: Items<i32, &'static str> = Items::ready(vec![1, 2, 3]);
        let mapped = items.try_map_item(|item| if item == 2 { Err("bad") } else { Ok(item * 2) });
        let batches = mapped.into_batches().collect::<Vec<_>>();

        assert_eq!(batches.len(), 3);
        assert_eq!(batches[0].as_ref().unwrap().as_slice(), &[2]);
        assert_eq!(batches[1], Err("bad"));
        assert_eq!(batches[2].as_ref().unwrap().as_slice(), &[6]);
    }

    #[test]
    fn drain_consumes_all_batches_before_returning_first_error() {
        let seen = Arc::new(AtomicUsize::new(0));
        let seen_iter = Arc::clone(&seen);
        let items: Items<i32, &'static str> = Items::stream(
            [Err("first"), Ok(vec![1]), Err("second")]
                .into_iter()
                .inspect(move |_| {
                    seen_iter.fetch_add(1, Ordering::SeqCst);
                }),
        );

        assert_eq!(items.drain(), Err("first"));
        assert_eq!(seen.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn drain_until_error_stops_at_first_error() {
        let seen = Arc::new(AtomicUsize::new(0));
        let seen_iter = Arc::clone(&seen);
        let items: Items<i32, &'static str> = Items::stream(
            [Err("first"), Ok(vec![1]), Err("second")]
                .into_iter()
                .inspect(move |_| {
                    seen_iter.fetch_add(1, Ordering::SeqCst);
                }),
        );

        assert_eq!(items.drain_until_error(), Err("first"));
        assert_eq!(seen.load(Ordering::SeqCst), 1);
    }
}
