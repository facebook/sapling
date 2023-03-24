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

use futures::Stream;
use pin_project::pin_project;

#[pin_project]
pub struct Monitor<T, P> {
    #[pin]
    inner: T,
    /// Payload alongside the inner stream or future.  This is dropped when
    /// the stream or future is dropped.
    #[allow(unused)]
    payload: P,
}

impl<T, P> Monitor<T, P> {
    pub fn new(inner: T, payload: P) -> Self {
        Self { inner, payload }
    }
}

impl<F: Future, P> Future for Monitor<F, P> {
    type Output = F::Output;

    #[inline]
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        this.inner.as_mut().poll(cx)
    }
}

impl<S: Stream, P> Stream for Monitor<S, P> {
    type Item = S::Item;

    #[inline]
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        this.inner.as_mut().poll_next(cx)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}
