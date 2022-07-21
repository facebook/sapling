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
use tokio::task::JoinError;
use tokio::task::JoinHandle;

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

/// Handle the `JoinError` you can get by awaiting a spawned task
/// Panics if the task was cancelled (should not happen in this crate)
/// Forwards panics back out if the task panicked
/// Or returns the contained result directly
pub(crate) fn handle_join_error<T>(res: Result<T, JoinError>) -> T {
    res.unwrap_or_else(|join_err| {
        if join_err.is_cancelled() {
            panic!("Unexpected use of JoinHandle::abort in bounded_traversal crate");
        }
        if !join_err.is_panic() {
            panic!("Cannot handle join error for a failure that's neither a panic nor a cancellation (tokio API has changed?)");
        }
        std::panic::resume_unwind(join_err.into_panic());
    })
}

pub(crate) struct DelayedSpawn<F>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    inner: DelayedSpawnInner<F>,
}

enum DelayedSpawnInner<F>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    Waiting(F),
    Spawning,
    Running(JoinHandle<F::Output>),
}

impl<F> DelayedSpawnInner<F>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    fn is_waiting(&self) -> bool {
        match self {
            Self::Waiting(_) => true,
            _ => false,
        }
    }
}

impl<F> Future for DelayedSpawn<F>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    type Output = Result<F::Output, JoinError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let inner = unsafe { &mut self.get_unchecked_mut().inner };
        if inner.is_waiting() {
            if let DelayedSpawnInner::Waiting(f) =
                std::mem::replace(inner, DelayedSpawnInner::Spawning)
            {
                *inner = DelayedSpawnInner::Running(tokio::spawn(f));
            }
        }

        if let DelayedSpawnInner::Running(h) = inner {
            Pin::new(h).poll(cx)
        } else {
            // SAFETY: Should never occur, as value should have been set to `Running`
            // above in all cases.
            panic!("DelayedSpawn in invalid state: not running and not waiting");
        }
    }
}

pub(crate) fn delay_spawn<F>(f: F) -> DelayedSpawn<F>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    DelayedSpawn {
        inner: DelayedSpawnInner::Waiting(f),
    }
}
