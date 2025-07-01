/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is dual-licensed under either the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree or the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree. You may select, at your option, one of the
 * above-listed licenses.
 */

use std::pin::Pin;

use futures::Future;
use futures::channel::oneshot::Canceled;
use futures::channel::oneshot::Receiver;
use futures::task::Context;
use futures::task::Poll;
use pin_project::pin_project;
use thiserror::Error;

/// This is a wrapper around [Receiver] that will return error when the receiver was polled
/// and the result was not ready. This is a very strict way of preventing deadlocks in code when
/// receiver is polled before the sender has send the result
#[pin_project]
pub struct ConservativeReceiver<T>(#[pin] Receiver<T>);

impl<T> ConservativeReceiver<T> {
    /// Return an instance of [ConservativeReceiver] wrapping the [Receiver]
    pub fn new(recv: Receiver<T>) -> Self {
        ConservativeReceiver(recv)
    }
}

impl<T> Future for ConservativeReceiver<T> {
    type Output = Result<T, ConservativeReceiverError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();

        match this.0.as_mut().poll(cx) {
            Poll::Ready(Ok(output)) => Poll::Ready(Ok(output)),
            Poll::Ready(Err(Canceled)) => Poll::Ready(Err(ConservativeReceiverError::Canceled)),
            Poll::Pending => Poll::Ready(Err(ConservativeReceiverError::ReceiveBeforeSend)),
        }
    }
}

/// Error that can be returned by [ConservativeReceiver]
#[derive(Error, Debug)]
pub enum ConservativeReceiverError {
    /// The underlying [Receiver] returned [Canceled]
    #[error("oneshot canceled")]
    Canceled,
    /// The underlying [Receiver] returned [Poll::Pending], which means it was polled
    /// before the [::futures::oneshot::Sender] send some data
    #[error("recv called on channel before send")]
    ReceiveBeforeSend,
}

#[cfg(test)]
mod test {
    use assert_matches::assert_matches;
    use futures::channel::oneshot::channel;

    use super::*;

    #[tokio::test]
    async fn recv_after_send() {
        let (send, recv) = channel();
        let recv = ConservativeReceiver::new(recv);

        send.send(42).expect("Failed to send");
        assert_matches!(recv.await, Ok(42));
    }

    #[tokio::test]
    async fn recv_before_send() {
        let (send, recv) = channel();
        let recv = ConservativeReceiver::new(recv);

        assert_matches!(
            recv.await,
            Err(ConservativeReceiverError::ReceiveBeforeSend)
        );
        send.send(42).expect_err("Should fail to send");
    }

    #[tokio::test]
    async fn recv_canceled_send() {
        let (_, recv) = channel::<()>();
        let recv = ConservativeReceiver::new(recv);

        assert_matches!(recv.await, Err(ConservativeReceiverError::Canceled));
    }
}
