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
    stream::Stream,
    task::{Context, Poll},
};

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
pub struct SignalStream<S> {
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

    fn pin_get_parts(self: Pin<&mut Self>) -> (Pin<&mut S>, &mut Option<Sender<u64>>, &mut u64) {
        // Pinning is structural for stream, non-structural for sender and size_sent.
        let this = unsafe { self.get_unchecked_mut() };
        let stream = unsafe { Pin::new_unchecked(&mut this.stream) };
        (stream, &mut this.sender, &mut this.size_sent)
    }

    fn pin_drop(self: Pin<&mut Self>) {
        let (_, sender, size_sent) = self.pin_get_parts();

        if let Some(sender) = sender.take() {
            let _ = sender.send(*size_sent);
        }
    }
}

impl<S, I, E> Stream for SignalStream<S>
where
    S: Stream<Item = Result<I, E>>,
    I: Sizeable,
{
    type Item = Result<I, E>;

    fn poll_next(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Option<Self::Item>> {
        let (stream, sender, size_sent) = self.pin_get_parts();

        if sender.is_none() {
            return Poll::Ready(None);
        }

        let poll = match stream.poll_next(ctx) {
            Poll::Ready(poll) => poll,
            Poll::Pending => {
                return Poll::Pending;
            }
        };

        if let Some(Ok(ref item)) = poll {
            // We have an item: increment the amount of data we sent.
            *size_sent += item.size();
        } else {
            // No items left: signal our receiver.
            let _ = sender
                .take()
                .expect("presence checked above")
                .send(*size_sent);
        }

        Poll::Ready(poll)
    }
}

impl<S> Drop for SignalStream<S> {
    fn drop(&mut self) {
        // `new_unchecked` is okay because we know this value is never used again after being
        // dropped.
        unsafe { Pin::new_unchecked(self) }.pin_drop();
    }
}
