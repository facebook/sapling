// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use futures::{try_ready, Async, Future, Poll};

#[derive(Clone, Copy)]
pub(crate) struct NodeLocation<Index> {
    pub node_index: Index,  // node index inside execution tree
    pub child_index: usize, // index inside parents children list
}

// This is essentially just a `.map`  over futures `{FFut|UFut}`, this only exisists
// so it would be possible to name `FuturesUnoredered` type parameter.
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
    FFut: Future<Error = UFut::Error>,
{
    type Item = JobResult<In, UFut::Item, FFut::Item>;
    type Error = FFut::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let result = match self {
            Job::Fold { value, future } => JobResult::Fold {
                value: value.clone(),
                result: try_ready!(future.poll()),
            },
            Job::Unfold { value, future } => JobResult::Unfold {
                value: value.clone(),
                result: try_ready!(future.poll()),
            },
        };
        Ok(Async::Ready(result))
    }
}
