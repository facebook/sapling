/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use futures::ready;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

#[derive(Clone, Copy)]
pub(crate) struct NodeLocation<Index> {
    pub node_index: Index,  // node index inside execution tree
    pub child_index: usize, // index inside parents children list
}

// This is essentially just a `.map`  over futures `{FFut|UFut}`, this only exisists
// so it would be possible to name `FuturesUnoredered` type parameter.
#[must_use = "futures do nothing unless you `.await` or poll them"]
pub(crate) enum Job<In, UFut, FFut> {
    Unfold { value: In, future: UFut },
    Fold { value: In, future: FFut },
}

pub(crate) enum JobResult<In, UFutResult, FFutResult> {
    Unfold { value: In, result: UFutResult },
    Fold { value: In, result: FFutResult },
}

impl<In, UFut, FFut> Future for Job<In, UFut, FFut>
where
    In: Clone,
    UFut: Future,
    FFut: Future,
{
    type Output = JobResult<In, UFut::Output, FFut::Output>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        // see `impl<A, B> Future for Either<A, B>`
        unsafe {
            let result = match self.get_unchecked_mut() {
                Job::Fold { value, future } => JobResult::Fold {
                    value: value.clone(),
                    result: ready!(Pin::new_unchecked(future).poll(cx)),
                },
                Job::Unfold { value, future } => JobResult::Unfold {
                    value: value.clone(),
                    result: ready!(Pin::new_unchecked(future).poll(cx)),
                },
            };
            Poll::Ready(result)
        }
    }
}
