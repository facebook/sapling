/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use futures_old::Future;
use futures_old::Poll;
use futures_old::Stream;

pub struct Monitor<T, P> {
    inner: T,
    // We don't actually do anything with this. We just rely on the fact that upon this Monitor
    // being dropped, P will be dropped.
    #[allow(dead_code)]
    payload: P,
}

impl<T, P> Monitor<T, P> {
    pub fn new(inner: T, payload: P) -> Self {
        Self { inner, payload }
    }
}

impl<F: Future, P> Future for Monitor<F, P> {
    type Item = F::Item;
    type Error = F::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.inner.poll()
    }
}

impl<S: Stream, P> Stream for Monitor<S, P> {
    type Item = S::Item;
    type Error = S::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.inner.poll()
    }
}
