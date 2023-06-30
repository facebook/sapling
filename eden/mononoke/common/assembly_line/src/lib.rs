/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(unboxed_closures)]
#![feature(fn_traits)]
#![feature(type_alias_impl_trait)]

use std::marker::PhantomData;

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

/// This overly complicated struct and FnMut implementations are used for
/// TryAssemblyLine because we need to name some types as we can't use
/// impl Trait in trait outputs in Rust yet.
pub struct ResultWrapper<F, I, T, Err>(F, PhantomData<(I, T, Err)>)
where
    F: FnMut<(I,)>,
    F::Output: Future<Output = Result<T, Err>>;

impl<F, I, T, Err> FnOnce<(Result<I, Err>,)> for ResultWrapper<F, I, T, Err>
where
    F: FnMut<(I,)>,
    F::Output: Future<Output = Result<T, Err>>,
{
    type Output = future::Either<F::Output, future::Ready<Result<T, Err>>>;
    extern "rust-call" fn call_once(mut self, args: (Result<I, Err>,)) -> Self::Output {
        self.call_mut(args)
    }
}

impl<F, I, T, Err> FnMut<(Result<I, Err>,)> for ResultWrapper<F, I, T, Err>
where
    F: FnMut<(I,)>,
    F::Output: Future<Output = Result<T, Err>>,
{
    extern "rust-call" fn call_mut(&mut self, (res,): (Result<I, Err>,)) -> Self::Output {
        match res {
            Ok(ok) => self.0.call_mut((ok,)).left_future(),
            Err(err) => future::ready(Err(err)).right_future(),
        }
    }
}

pub trait TryAssemblyLine: TryStream + Sized {
    fn try_next_step<F, O>(
        self,
        step_fn: F,
    ) -> NextStep<Self, ResultWrapper<F, Self::Ok, O, Self::Error>>
    where
        F: FnMut<(Self::Ok,)>,
        F::Output: Future<Output = Result<O, Self::Error>>,
        Self: FusedStream,
        // This is always true, not sure why I need this bound
        Self: Stream<Item = Result<Self::Ok, Self::Error>>,
    {
        NextStep::new(self, ResultWrapper(step_fn, PhantomData))
    }
}

impl<S: TryStream> TryAssemblyLine for S {}
