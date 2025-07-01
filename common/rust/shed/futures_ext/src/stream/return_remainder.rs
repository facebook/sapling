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

use futures::Stream;
use futures::channel::oneshot::Sender;
use futures::channel::oneshot::channel;
use futures::ready;
use futures::task::Context;
use futures::task::Poll;
use pin_project::pin_project;

use crate::future::ConservativeReceiver;

/// A stream wrapper returned by StreamExt::return_remainder
#[pin_project]
pub struct ReturnRemainder<In> {
    #[pin]
    inner: Option<In>,
    send: Option<Sender<In>>,
}

impl<In> ReturnRemainder<In> {
    pub(crate) fn new(inner: In) -> (Self, ConservativeReceiver<In>) {
        let (send, recv) = channel();
        (
            Self {
                inner: Some(inner),
                send: Some(send),
            },
            ConservativeReceiver::new(recv),
        )
    }
}

impl<In: Stream + Unpin> Stream for ReturnRemainder<In> {
    type Item = In::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        let maybe_item = match this.inner.as_mut().as_pin_mut() {
            Some(inner) => ready!(inner.poll_next(cx)),
            None => return Poll::Ready(None),
        };

        if maybe_item.is_none() {
            let inner = this
                .inner
                .get_mut()
                .take()
                .expect("inner was just polled, should be some");
            let send = this.send.take().expect("send is None iff inner is None");
            // The Receiver will handle errors
            let _ = send.send(inner);
        }

        Poll::Ready(maybe_item)
    }
}

#[cfg(test)]
mod test {
    use assert_matches::assert_matches;
    use futures::future;
    use futures::stream::StreamExt;
    use futures::stream::iter;

    use super::*;

    #[tokio::test]
    async fn success_get_remainder() {
        let s = iter(1..=10).take_while(|x| future::ready(*x <= 5));
        let (s, rest) = ReturnRemainder::new(s);
        assert_eq!(vec![1, 2, 3, 4, 5], s.collect::<Vec<_>>().await);

        let rest = rest.await.expect("Failed to get what is left");
        assert_eq!(
            vec![7, 8, 9, 10],
            rest.into_inner().collect::<Vec<_>>().await
        );
    }

    #[tokio::test]
    async fn fail_get_remainder() {
        let s = iter(1..=10).take_while(|x| future::ready(*x <= 5));
        let (s, rest) = ReturnRemainder::new(s);

        assert_matches!(rest.await, Err(_));
        assert_eq!(vec![1, 2, 3, 4, 5], s.collect::<Vec<_>>().await);
    }
}
