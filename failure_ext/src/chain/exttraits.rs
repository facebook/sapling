// Copyright 2004-present Facebook. All Rights Reserved.

use boxfnonce::SendBoxFnOnce;
use futures::{Future, Poll, Stream};

use super::Chain;
use crate::{Error, Fail};

// Dummy types to distinguish different trait implementations, since we can't do
// a blanket implementation for all `F: Fail` without getting conherence rule failures
// for other types which might implement `Fail` in future.
pub enum MarkerFail {} // Any F where F: Fail
pub enum MarkerError {} // Error
pub enum MarkerResultFail {} // Result<T, F> where F: Fail
pub enum MarkerResultError {} // Result<T, Error>
pub enum MarkerFutureFail {} // Future<Error=F> where F: Fail
pub enum MarkerFutureError {} // Future<Error=Error>
pub enum MarkerStreamFail {} // Stream<Error=F> where F: Fail
pub enum MarkerStreamError {} // Stream<Error=Error>
pub enum MarkerChainFail {} // Chain for F: Fail
pub enum MarkerChainError {} // Chain for Error

/// Extension of Error to wrap an error in a higher-level error. This is similar to
/// failure::Context, but it is explicitly intended to maintain causal chains of errors.
pub trait ChainExt<MARKER, ERR> {
    type Chained;

    fn chain_err(self, outer_err: ERR) -> Self::Chained;
}

impl<ERR> ChainExt<MarkerError, ERR> for Error {
    type Chained = Chain<ERR>;

    fn chain_err(self, err: ERR) -> Chain<ERR> {
        Chain::with_error(err, self)
    }
}

impl<F, ERR> ChainExt<MarkerFail, ERR> for F
where
    F: Fail,
{
    type Chained = Chain<ERR>;

    fn chain_err(self, err: ERR) -> Chain<ERR> {
        Chain::with_fail(err, self)
    }
}

impl<T, ERR> ChainExt<MarkerResultError, ERR> for Result<T, Error> {
    type Chained = Result<T, Chain<ERR>>;

    fn chain_err(self, err: ERR) -> Result<T, Chain<ERR>> {
        self.map_err(|cause| Chain::with_error(err, cause))
    }
}

impl<T, F, ERR> ChainExt<MarkerResultFail, ERR> for Result<T, F>
where
    F: Fail,
{
    type Chained = Result<T, Chain<ERR>>;

    fn chain_err(self, err: ERR) -> Result<T, Chain<ERR>> {
        self.map_err(|cause| Chain::with_fail(err, cause))
    }
}

pub struct ChainFuture<F, ERR>
where
    F: Future,
{
    chain: Option<SendBoxFnOnce<'static, (F::Error,), Chain<ERR>>>,
    future: F,
}

impl<F, ERR> Future for ChainFuture<F, ERR>
where
    F: Future,
{
    type Item = F::Item;
    type Error = Chain<ERR>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.future.poll() {
            Err(err) => Err(self
                .chain
                .take()
                .expect("ChainFuture called after error completion")
                .call(err)),
            Ok(ok) => Ok(ok),
        }
    }
}

impl<F, ERR> ChainExt<MarkerFutureError, ERR> for F
where
    F: Future<Error = Error>,
    ERR: Send + 'static,
{
    type Chained = ChainFuture<F, ERR>;

    fn chain_err(self, err: ERR) -> ChainFuture<F, ERR> {
        ChainFuture {
            chain: Some(SendBoxFnOnce::from(move |cause| {
                Chain::with_error(err, cause)
            })),
            future: self,
        }
    }
}

impl<F, ERR> ChainExt<MarkerFutureFail, ERR> for F
where
    F: Future,
    F::Error: Fail,
    ERR: Send + 'static,
{
    type Chained = ChainFuture<F, ERR>;

    fn chain_err(self, err: ERR) -> ChainFuture<F, ERR> {
        ChainFuture {
            chain: Some(SendBoxFnOnce::from(move |cause| {
                Chain::with_fail(err, cause)
            })),
            future: self,
        }
    }
}

pub struct ChainStream<S, ERR>
where
    S: Stream,
{
    chain: Option<SendBoxFnOnce<'static, (S::Error,), Chain<ERR>>>,
    stream: S,
}

impl<S, ERR> Stream for ChainStream<S, ERR>
where
    S: Stream,
{
    type Item = S::Item;
    type Error = Chain<ERR>;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.stream.poll() {
            Err(err) => Err(self
                .chain
                .take()
                .expect("ChainStream called after error completion")
                .call(err)),
            Ok(ok) => Ok(ok),
        }
    }
}

impl<S, ERR> ChainExt<MarkerStreamError, ERR> for S
where
    S: Stream<Error = Error>,
    ERR: Send + 'static,
{
    type Chained = ChainStream<S, ERR>;

    fn chain_err(self, err: ERR) -> ChainStream<S, ERR> {
        ChainStream {
            chain: Some(SendBoxFnOnce::from(move |cause| {
                Chain::with_error(err, cause)
            })),
            stream: self,
        }
    }
}

impl<S, ERR> ChainExt<MarkerStreamFail, ERR> for S
where
    S: Stream,
    S::Error: Fail,
    ERR: Send + 'static,
{
    type Chained = ChainStream<S, ERR>;

    fn chain_err(self, err: ERR) -> ChainStream<S, ERR> {
        ChainStream {
            chain: Some(SendBoxFnOnce::from(move |cause| {
                Chain::with_fail(err, cause)
            })),
            stream: self,
        }
    }
}
