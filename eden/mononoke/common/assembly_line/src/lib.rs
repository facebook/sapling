/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(unboxed_closures)]

use futures::future;
use futures::stream::FusedStream;
use futures::Future;
use futures::FutureExt;
use futures::Stream;
use futures::TryStream;

mod next_step;

pub use next_step::NextStep;

pub trait AssemblyLine: Stream + Sized {
    fn next_step<F>(self, step_fn: F) -> NextStep<Self, F>
    where
        F: FnMut<(Self::Item,)>,
        F::Output: Future,
        Self: FusedStream,
    {
        NextStep::new(self, step_fn)
    }
}

impl<S: Stream> AssemblyLine for S {}

pub struct TryAssemblyLine;

impl TryAssemblyLine {
    pub fn try_next_step<S, F, O>(
        stream: S,
        mut step_fn: F,
    ) -> impl Stream<Item = Result<O, S::Error>>
    where
        S: TryStream + FusedStream,
        F: FnMut<(S::Ok,)>,
        F::Output: Future<Output = Result<O, S::Error>>,
        // This is always true, not sure why I need this bound
        S: Stream<Item = Result<S::Ok, S::Error>>,
    {
        NextStep::new(stream, move |res| match res {
            Ok(ok) => step_fn(ok).left_future(),
            Err(err) => future::ready(Err(err)).right_future(),
        })
    }
}
