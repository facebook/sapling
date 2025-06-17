/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

//! Module extending functionality of [`futures::future`] module

mod abort_handle_ref;
mod conservative_receiver;
mod on_cancel;
mod on_cancel_with_data;
mod try_shared;

use std::time::Duration;

use anyhow::Error;
use futures::future::Future;
use futures::future::FutureExt;
use futures::future::TryFuture;
pub use shared_error::anyhow::SharedError;
use tokio::time::Timeout;

pub use self::abort_handle_ref::ControlledHandle;
pub use self::abort_handle_ref::spawn_controlled;
pub use self::conservative_receiver::ConservativeReceiver;
pub use self::on_cancel::OnCancel;
pub use self::on_cancel_with_data::CancelData;
pub use self::on_cancel_with_data::OnCancelWithData;
pub use self::try_shared::TryShared;

/// A trait implemented by default for all Futures which extends the standard
/// functionality.
pub trait FbFutureExt: Future {
    /// Construct a new [tokio::time::Timeout].
    fn timeout(self, timeout: Duration) -> Timeout<Self>
    where
        Self: Sized,
    {
        tokio::time::timeout(timeout, self)
    }

    /// Call the `on_cancel` callback if this future is canceled (dropped
    /// without completion).
    fn on_cancel<F: FnOnce()>(self, on_cancel: F) -> OnCancel<Self, F>
    where
        Self: Sized,
    {
        OnCancel::new(self, on_cancel)
    }

    /// Call the `on_cancel` callback if this future is canceled (dropped
    /// without completion).  Pass additional data extracted from the
    /// inner future via the CancelData trait.
    fn on_cancel_with_data<F>(self, on_cancel: F) -> OnCancelWithData<Self, F>
    where
        Self: Sized + CancelData,
        F: FnOnce(Self::Data),
    {
        OnCancelWithData::new(self, on_cancel)
    }
}

impl<T> FbFutureExt for T where T: Future + ?Sized {}

/// A trait implemented by default for all Futures which extends the standard
/// functionality.
pub trait FbTryFutureExt: Future {
    /// Create a cloneable handle to this future where all handles will resolve
    /// to the same result.
    ///
    /// Similar to [futures::future::Shared], but instead works on Futures
    /// returning Result where Err is [anyhow::Error].
    /// This is achieved by storing [anyhow::Error] in [std::sync::Arc].
    fn try_shared(self) -> TryShared<Self>
    where
        Self: TryFuture<Error = Error> + Sized,
        <Self as TryFuture>::Ok: Clone,
    {
        self::try_shared::try_shared(self)
    }

    /// Convert a `Future` of `Result<Result<I, E1>, E2>` into a `Future` of
    /// `Result<I, E1>`, assuming `E2` can convert into `E1`.
    #[allow(clippy::type_complexity)]
    fn flatten_err<I, E1, E2>(
        self,
    ) -> futures::future::Map<Self, fn(Result<Result<I, E1>, E2>) -> Result<I, E1>>
    where
        Self: Sized,
        Self: Future<Output = Result<Result<I, E1>, E2>>,
        E1: From<E2>,
    {
        fn flatten_err<I, E1, E2>(e: Result<Result<I, E1>, E2>) -> Result<I, E1>
        where
            E1: From<E2>,
        {
            match e {
                Ok(Ok(i)) => Ok(i),
                Ok(Err(e1)) => Err(e1),
                Err(e2) => Err(E1::from(e2)),
            }
        }

        self.map(flatten_err)
    }
}

impl<T> FbTryFutureExt for T where T: TryFuture + ?Sized {}
