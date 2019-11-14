/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use failure::{Error, Fallible as Result};
use futures::future::poll_fn;
use tokio::prelude::*;
use tokio_threadpool::blocking;

use cloned::cloned;

struct AsyncWrapperInner<T> {
    data: T,
}

/// Wraps a synchronous datastructure into an asynchronous one.
pub struct AsyncWrapper<T> {
    inner: Arc<AsyncWrapperInner<T>>,
}

impl<T: Send + Sync> AsyncWrapper<T> {
    pub fn new(data: T) -> Self {
        AsyncWrapper {
            inner: Arc::new(AsyncWrapperInner { data }),
        }
    }

    /// Wraps callback into a blocking context.
    pub fn block<U: Send>(
        &self,
        callback: impl Fn(&T) -> Result<U> + Send,
    ) -> impl Future<Item = U, Error = Error> + Send {
        poll_fn({
            cloned!(self.inner);
            move || blocking(|| callback(&inner.data))
        })
        .from_err()
        .and_then(|res| res)
    }
}
