/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::pin::Pin;

use anyhow::Error;
use bytes::Bytes;
use futures::channel::oneshot::Sender;
use futures::ready;
use futures::stream::Stream;
use futures::task::Context;
use futures::task::Poll;
use pin_project::pin_project;
use pin_project::pinned_drop;

use super::error_meta::ErrorMeta;
use super::error_meta::ErrorMetaProvider;
use super::response_meta::BodyMeta;

pub trait Sizeable {
    fn size(&self) -> u64;
}

impl Sizeable for Bytes {
    fn size(&self) -> u64 {
        // NOTE: It is reasonable to unwrap here because we're not going to have buffers of bytes
        // that are larger than a u64.
        self.len().try_into().unwrap()
    }
}

/// A stream that will fire to the sender associated upon completing or being dropped. The Sender
/// will receive the amount of data that passed through the stream.
#[pin_project(PinnedDrop, project = SignalStreamProjection)]
pub struct SignalStream<S: ErrorMetaProvider<Error>> {
    #[pin]
    stream: S,
    sender: Option<Sender<BodyMeta>>,
    size_sent: u64,
}

impl<S> SignalStream<S>
where
    S: ErrorMetaProvider<Error>,
{
    pub fn new(stream: S, sender: Sender<BodyMeta>) -> Self {
        Self {
            stream,
            sender: Some(sender),
            size_sent: 0,
        }
    }
}

impl<S, T> Stream for SignalStream<S>
where
    S: Stream<Item = T> + ErrorMetaProvider<Error>,
    T: Sizeable,
{
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        let poll = ready!(this.stream.as_mut().poll_next(ctx));

        match poll {
            // We have an item: increment the amount of data we sent.
            Some(ref item) => *this.size_sent += item.size(),
            // No items left: signal our receiver.
            None => send_body_meta(this),
        }

        Poll::Ready(poll)
    }
}

#[pinned_drop]
impl<S> PinnedDrop for SignalStream<S>
where
    S: ErrorMetaProvider<Error>,
{
    fn drop(self: Pin<&mut Self>) {
        let this = self.project();
        send_body_meta(this);
    }
}

fn send_body_meta<S>(this: SignalStreamProjection<S>)
where
    S: ErrorMetaProvider<Error>,
{
    if let Some(sender) = this.sender.take() {
        let bytes_sent = *this.size_sent;
        let mut error_meta = ErrorMeta::new();

        this.stream.report_errors(&mut error_meta);

        let _ = sender.send(BodyMeta {
            bytes_sent,
            error_meta,
        });
    }
}
