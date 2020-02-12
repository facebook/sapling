/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use futures::{
    ready,
    stream::{self, FuturesUnordered, StreamExt},
    Stream,
};
use std::{collections::VecDeque, future::Future, iter::FromIterator, task::Poll};

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
pub fn bounded_traversal_stream<In, InsInit, Ins, Out, Unfold, UFut, UErr>(
    scheduled_max: usize,
    init: InsInit,
    mut unfold: Unfold,
) -> impl Stream<Item = Result<Out, UErr>>
where
    Unfold: FnMut(In) -> UFut,
    // TODO UFut could be IntoFuture once https://github.com/rust-lang/rust/pull/65244 is visible
    UFut: Future<Output = Result<(Out, Ins), UErr>>,
    InsInit: IntoIterator<Item = In>,
    Ins: IntoIterator<Item = In>,
{
    let mut unscheduled = VecDeque::from_iter(init);
    let mut scheduled = FuturesUnordered::new();
    stream::poll_fn(move |cx| loop {
        if scheduled.is_empty() && unscheduled.is_empty() {
            return Poll::Ready(None);
        }

        for item in
            unscheduled.drain(..std::cmp::min(unscheduled.len(), scheduled_max - scheduled.len()))
        {
            scheduled.push(unfold(item))
        }

        if let Some((out, children)) = ready!(scheduled.poll_next_unpin(cx)).transpose()? {
            for child in children {
                unscheduled.push_front(child);
            }
            return Poll::Ready(Some(Ok(out)));
        }
    })
}
