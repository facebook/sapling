/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::future::Future;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use either::Either;
use futures::ready;

/// Return value from `unfold` callbacks for ordered traversals.  Each element
/// in the unfolded vector can be either an item to output from the traversal,
/// or a recursive step with an associated weight.
///
/// The associated weight should be an estimate of the number of eventual
/// output items this recursive step will expand into.
pub enum OrderedTraversal<Out, In> {
    Output(Out),
    Recurse(usize, In),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NodeLocation<Index> {
    pub node_index: Index,  // node index inside execution tree
    pub child_index: usize, // index inside parents children list
}

impl<Index> NodeLocation<Index> {
    pub fn new(node_index: Index, child_index: usize) -> Self {
        NodeLocation {
            node_index,
            child_index,
        }
    }
}

/// Equivalent of `futures::future::Either` but with heterogeneous output
/// types using `either::Either`.
#[must_use = "futures do nothing unless you `.await` or poll them"]
pub(crate) enum Either2<A, B> {
    Left(A),
    Right(B),
}

impl<A, B> Future for Either2<A, B>
where
    A: Future,
    B: Future,
{
    type Output = Either<A::Output, B::Output>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        // see `impl<Left, Right> Future for Either<Left, Right>`
        unsafe {
            let result = match self.get_unchecked_mut() {
                Either2::Left(future) => Either::Left(ready!(Pin::new_unchecked(future).poll(cx))),
                Either2::Right(future) => {
                    Either::Right(ready!(Pin::new_unchecked(future).poll(cx)))
                }
            };
            Poll::Ready(result)
        }
    }
}
