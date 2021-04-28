/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::pin::Pin;

use futures::{
    ready,
    stream::{Stream, TryStream},
    task::{Context, Poll},
};
use pin_project::pin_project;

pub trait UpgradeBytesExt: TryStream {
    /// Convert a Stream of Bytes 0.5 to one of Bytes 1.0.
    fn upgrade_bytes(self) -> UpgradeBytes<Self>
    where
        Self: Sized,
    {
        UpgradeBytes::new(self)
    }
}

impl<S: TryStream + ?Sized> UpgradeBytesExt for S {}

#[pin_project]
pub struct UpgradeBytes<S> {
    #[pin]
    inner: S,
}

impl<S> UpgradeBytes<S> {
    pub fn new(inner: S) -> Self {
        Self { inner }
    }

    pub fn get_ref(&self) -> &S {
        &self.inner
    }
}

impl<S, E> Stream for UpgradeBytes<S>
where
    S: Stream<Item = Result<bytes_05::Bytes, E>>,
{
    type Item = Result<bytes::Bytes, E>;

    fn poll_next(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let projection = self.project();
        let next = ready!(projection.inner.poll_next(ctx));
        let next = next.map(|n| n.map(|b| bytes::Bytes::copy_from_slice(b.as_ref())));
        Poll::Ready(next)
    }
}
