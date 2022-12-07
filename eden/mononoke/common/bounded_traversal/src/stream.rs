/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::VecDeque;
use std::hash::Hash;
use std::task::Poll;

use futures::future::BoxFuture;
use futures::ready;
use futures::stream;
use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;
use futures::Stream;

/// `bounded_traversal_stream` traverses implicit asynchronous tree specified by `init`
/// and `unfold` arguments. All `unfold` operations are executed in parallel if they
/// do not depend on each other (not related by ancestor-descendant relation in implicit
/// tree) with amount of concurrency constrained by `scheduled_max`. Main difference
/// with `bounded_traversal` is that this one is not structure perserving, and returns
/// stream.
///
/// ## `init: InsInit`
/// Is the root(s) of the implicit tree to be traversed
///
/// ## `unfold: FnMut(In) -> impl Future<Output = Result<(Out, impl IntoIterator<Item = In>), UErr>>`
/// Asynchronous function which given input value produces list of its children and output
/// value.
///
/// ## return value `impl Stream<Item = Result<Out, UErr>>`
/// Stream of all `Out` values
///
pub fn bounded_traversal_stream<'caller, In, InsInit, Ins, Out, Unfold, UErr>(
    scheduled_max: usize,
    init: InsInit,
    mut unfold: Unfold,
) -> impl Stream<Item = Result<Out, UErr>> + 'caller
where
    In: 'caller,
    Out: 'caller,
    UErr: 'caller,
    // We use BoxFuture here because the `Unfold` future can be very large.
    // As a result, it's more efficient to keep it in one place (the heap)
    // than to move it around on the stack all the time.
    // https://fburl.com/m3cdcdko
    Unfold: FnMut(In) -> BoxFuture<'caller, Result<(Out, Ins), UErr>> + 'caller,
    InsInit: IntoIterator<Item = In> + 'caller,
    Ins: IntoIterator<Item = In> + 'caller,
{
    let mut unscheduled = VecDeque::from_iter(init);
    let mut scheduled = FuturesUnordered::new();
    stream::poll_fn(move |cx| {
        loop {
            if scheduled.is_empty() && unscheduled.is_empty() {
                return Poll::Ready(None);
            }

            for item in unscheduled
                .drain(..std::cmp::min(unscheduled.len(), scheduled_max - scheduled.len()))
            {
                scheduled.push(unfold(item))
            }

            if let Some((out, children)) = ready!(scheduled.poll_next_unpin(cx)).transpose()? {
                for child in children {
                    unscheduled.push_front(child);
                }
                return Poll::Ready(Some(Ok(out)));
            }
        }
    })
}

/// This function is similar to `bounded_traversal_stream` but:
///   - prevents items with duplicate keys executing concurrently
///   - allows an item to have no stream output by returning None
///   - optionally allows to restrict the number of keys executing concurrently by a ShardKey
pub fn limited_by_key_shardable<In, InsInit, Ins, Out, Unfold, UErr, Key, KeyFn, ShardKey>(
    scheduled_max: usize,
    init: InsInit,
    mut unfold: Unfold,
    key_fn: KeyFn,
) -> impl Stream<Item = Result<Out, UErr>>
where
    // As above, we use BoxFuture here because the `Unfold` future can be very
    // large.  As a result, it's more efficient to keep it in one place (the
    // heap) than to move it around on the stack all the time.
    Unfold:
        FnMut(In) -> BoxFuture<'static, (Key, Option<ShardKey>, Result<Option<(Out, Ins)>, UErr>)>,
    InsInit: IntoIterator<Item = In>,
    Ins: IntoIterator<Item = In>,
    Key: Clone + Eq + Hash,
    KeyFn: Fn(&In) -> (&Key, Option<(ShardKey, usize)>),
    ShardKey: Clone + Eq + Hash,
{
    let mut unscheduled = VecDeque::from_iter(init);
    let mut scheduled = FuturesUnordered::new();
    let mut waiting_for_key: HashMap<Key, VecDeque<_>> = HashMap::new();
    let mut waiting_for_shard: HashMap<ShardKey, (usize, VecDeque<_>)> = HashMap::new();

    stream::poll_fn(move |cx| {
        loop {
            if scheduled.is_empty() && unscheduled.is_empty() {
                return Poll::Ready(None);
            }

            while scheduled.len() < scheduled_max && !unscheduled.is_empty() {
                for item in unscheduled
                    .drain(..std::cmp::min(unscheduled.len(), scheduled_max - scheduled.len()))
                {
                    let (key, shard_info) = key_fn(&item);
                    if let Some(inflight) = waiting_for_key.get_mut(key) {
                        // Exact duplicate, it needs to wait
                        inflight.push_back(item);
                        continue;
                    }

                    if let Some((shard_key, max_per_shard)) = shard_info {
                        let (inflight, queued) = waiting_for_shard.entry(shard_key).or_default();
                        if *inflight > max_per_shard {
                            // Shard is too busy, so queue more
                            queued.push_back(item);
                            continue;
                        } else {
                            *inflight += 1;
                        }
                    }

                    waiting_for_key.insert(key.clone(), VecDeque::new());
                    scheduled.push(unfold(item));
                }
            }

            if let Some((key, shard_key, unfolded)) = ready!(scheduled.poll_next_unpin(cx)) {
                if let Some((key, mut queue)) = waiting_for_key.remove_entry(&key) {
                    if let Some(item) = queue.pop_front() {
                        let unfolded = unfold(item);
                        scheduled.push(unfolded);
                    }
                    if !queue.is_empty() {
                        waiting_for_key.insert(key, queue);
                    }
                }

                if let Some(shard_key) = shard_key {
                    if let Some((inflight, queue)) = waiting_for_shard.get_mut(&shard_key) {
                        *inflight = inflight.saturating_sub(1);
                        if let Some(item) = queue.pop_front() {
                            // Don't directly schedule as could be a duplicate key
                            unscheduled.push_front(item);
                        }
                    }
                }

                if let Some((out, children)) = unfolded? {
                    // there is output on this unfold
                    for child in children {
                        unscheduled.push_front(child);
                    }
                    return Poll::Ready(Some(Ok(out)));
                }
            }
        }
    })
}
