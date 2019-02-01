// Copyright 2019 Facebook, Inc.

use std::sync::{Arc, Mutex};

use failure::{Error, Fallible};
use futures::future::poll_fn;
use tokio::prelude::*;
use tokio_threadpool::blocking;

use cloned::cloned;

struct AsyncWrapperInner<T> {
    data: T,
}

/// Wraps a synchronous datastructure into an asynchronous one.
pub struct AsyncWrapper<T> {
    inner: Arc<Mutex<AsyncWrapperInner<T>>>,
}

impl<T: Send> AsyncWrapper<T> {
    pub fn new(data: T) -> Self {
        AsyncWrapper {
            inner: Arc::new(Mutex::new(AsyncWrapperInner { data })),
        }
    }

    /// Wraps callback into a blocking context.
    pub fn block<U: Send>(
        &self,
        callback: impl Fn(&T) -> Fallible<U> + Send,
    ) -> impl Future<Item = U, Error = Error> + Send {
        poll_fn({
            cloned!(self.inner);
            move || {
                blocking(|| {
                    let inner = inner.lock().expect("Poisoned Mutex");
                    callback(&inner.data)
                })
            }
        })
        .from_err()
        .and_then(|res| res)
    }
}
