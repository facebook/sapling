// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use failure_ext::{format_err, Error};
use futures::{sync::oneshot, Future, IntoFuture};
use futures_ext::FutureExt;
use std::result::Result;
use tokio;

// Spawn provides a helper to dispatch futures to the Tokio executor yet retain a handle to their
// results in the form of a Future. We also provide methods to cast SpawnError into an Error for
// convenience.

#[derive(Debug)]
pub enum SpawnError<E> {
    Error(E),
    Cancelled,
}

impl Into<Error> for SpawnError<Error> {
    fn into(self) -> Error {
        use SpawnError::*;

        match self {
            Error(e) => e,
            e @ Cancelled => format_err!("SpawnError: {:?}", e),
        }
    }
}

impl Into<Error> for SpawnError<!> {
    fn into(self) -> Error {
        use SpawnError::*;

        match self {
            Error(e) => e,
            e @ Cancelled => format_err!("SpawnError: {:?}", e),
        }
    }
}

// NOTE: We don't use FutureExt's spawn_future here because we need the Future to start doing work
// immediately. That's because we used these spawned futures with the Multiplexer, which requires
// those futures to do progress for draining to complete.
pub fn spawn_and_start<F, I, E>(fut: F) -> impl Future<Item = I, Error = SpawnError<E>>
where
    F: Future<Item = I, Error = E> + Send + 'static,
    I: Send + 'static,
    E: Send + 'static,
{
    let (sender, receiver) = oneshot::channel::<Result<I, E>>();

    let fut = fut.then(|res| sender.send(res)).discard();
    tokio::spawn(fut);

    receiver.into_future().then(|res| match res {
        Ok(Ok(r)) => Ok(r),
        Ok(Err(e)) => Err(SpawnError::Error(e)),
        Err(_) => Err(SpawnError::Cancelled),
    })
}
