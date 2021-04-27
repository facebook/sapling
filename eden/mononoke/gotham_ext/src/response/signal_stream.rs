/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::TryInto;
use std::pin::Pin;

use bytes::Bytes;
use futures::{
    channel::oneshot::Sender,
    ready,
    stream::Stream,
    task::{Context, Poll},
};
use pin_project::{pin_project, pinned_drop};

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
#[pin_project(PinnedDrop)]
pub struct SignalStream<S> {
    #[pin]
    stream: S,
    sender: Option<Sender<u64>>,
    size_sent: u64,
}

impl<S> SignalStream<S> {
    pub fn new(stream: S, sender: Sender<u64>) -> Self {
        Self {
            stream,
            sender: Some(sender),
            size_sent: 0,
        }
    }
}

impl<S, T> Stream for SignalStream<S>
where
    S: Stream<Item = T>,
    T: Sizeable,
{
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Option<Self::Item>> {
        let this = self.project();

        let poll = ready!(this.stream.poll_next(ctx));

        match poll {
            // We have an item: increment the amount of data we sent.
            Some(ref item) => *this.size_sent += item.size(),
            // No items left: signal our receiver.
            None => {
                if let Some(sender) = this.sender.take() {
                    let _ = sender.send(*this.size_sent);
                }
            }
        }

        Poll::Ready(poll)
    }
}

#[pinned_drop]
impl<S> PinnedDrop for SignalStream<S> {
    fn drop(self: Pin<&mut Self>) {
        let this = self.project();

        if let Some(sender) = this.sender.take() {
            let _ = sender.send(*this.size_sent);
        }
    }
}
