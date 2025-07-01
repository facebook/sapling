/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is dual-licensed under either the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree or the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree. You may select, at your option, one of the
 * above-listed licenses.
 */

use anyhow::Error;
use futures::future;
use futures::future::FutureExt;
use futures::future::Shared;
use futures::future::TryFuture;
use futures::future::TryFutureExt;
use shared_error::anyhow::IntoSharedError;
use shared_error::anyhow::SharedError;

/// Type returned by the `try_shared` method provided by the `FbFutureExt` trait.
pub type TryShared<Fut> = Shared<future::MapErr<Fut, NewSharedError>>;

/// Type alias for easier definition of TryShared
type NewSharedError = fn(Error) -> SharedError;

pub(crate) fn try_shared<Fut>(fut: Fut) -> TryShared<Fut>
where
    <Fut as TryFuture>::Ok: Clone,
    Fut: TryFuture<Error = Error> + Sized,
{
    fut.map_err(IntoSharedError::<SharedError>::shared_error as NewSharedError)
        .shared()
}
