/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use either::Either;
use futures::ready;

#[derive(Clone, Copy)]
pub(crate) struct NodeLocation<Index> {
    pub node_index: Index,  // node index inside execution tree
    pub child_index: usize, // index inside parents children list
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
