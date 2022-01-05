/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Provides a version of `futures::stream::select` which drops each stream when it terminates.
//! This code is derived from `futures::stream::select`, with the streams wrapped in `Option`.

use std::pin::Pin;

use futures::stream::Fuse;
use futures::stream::FusedStream;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::task::Context;
use futures::task::Poll;
use pin_project::pin_project;

/// Stream for the [`select_drop()`] function.
#[pin_project]
#[derive(Debug)]
#[must_use = "streams do nothing unless polled"]
pub struct SelectDrop<St1, St2> {
    #[pin]
    stream1: Option<Fuse<St1>>,
    #[pin]
    stream2: Option<Fuse<St2>>,
    poll_stream2_first: bool,
}

/// This function will attempt to pull items from both streams. Each
/// stream will be polled in a round-robin fashion, and whenever a stream is
/// ready to yield an item that item is yielded.
///
/// After one of the two input stream completes, the remaining one will be
/// polled exclusively. The returned stream completes when both input
/// streams have completed.
///
/// Note that this function consumes both streams and returns a wrapped
/// version of them.
///
/// This function is identical to `futures::stream::select`, except that
/// it drops each stream once it terminates.
pub fn select_drop<St1, St2>(stream1: St1, stream2: St2) -> SelectDrop<St1, St2>
where
    St1: Stream,
    St2: Stream<Item = St1::Item>,
{
    SelectDrop {
        stream1: Some(stream1.fuse()),
        stream2: Some(stream2.fuse()),
        poll_stream2_first: false,
    }
}

impl<St1, St2> SelectDrop<St1, St2> {
    /// Acquires a reference to the underlying streams that this combinator is
    /// pulling from.
    pub fn get_ref(&self) -> (Option<&St1>, Option<&St2>) {
        (
            self.stream1.as_ref().map(|s| s.get_ref()),
            self.stream2.as_ref().map(|s| s.get_ref()),
        )
    }

    /// Acquires a mutable reference to the underlying streams that this
    /// combinator is pulling from.
    ///
    /// Note that care must be taken to avoid tampering with the state of the
    /// stream which may otherwise confuse this combinator.
    pub fn get_mut(&mut self) -> (Option<&mut St1>, Option<&mut St2>) {
        (
            self.stream1.as_mut().map(|s| s.get_mut()),
            self.stream2.as_mut().map(|s| s.get_mut()),
        )
    }

    /// Acquires a pinned mutable reference to the underlying streams that this
    /// combinator is pulling from.
    ///
    /// Note that care must be taken to avoid tampering with the state of the
    /// stream which may otherwise confuse this combinator.
    pub fn get_pin_mut(self: Pin<&mut Self>) -> (Option<Pin<&mut St1>>, Option<Pin<&mut St2>>) {
        let this = self.project();
        (
            this.stream1.as_pin_mut().map(|s| s.get_pin_mut()),
            this.stream2.as_pin_mut().map(|s| s.get_pin_mut()),
        )
    }

    /// Consumes this combinator, returning the underlying streams.
    ///
    /// Note that this may discard intermediate state of this combinator, so
    /// care should be taken to avoid losing resources when this is called.
    pub fn into_inner(self) -> (Option<St1>, Option<St2>) {
        (
            self.stream1.map(|s| s.into_inner()),
            self.stream2.map(|s| s.into_inner()),
        )
    }
}

impl<St1, St2> FusedStream for SelectDrop<St1, St2>
where
    St1: Stream,
    St2: Stream<Item = St1::Item>,
{
    fn is_terminated(&self) -> bool {
        self.stream1.as_ref().map_or(true, |s| s.is_terminated())
            && self.stream2.as_ref().map_or(true, |s| s.is_terminated())
    }
}

impl<St1, St2> Stream for SelectDrop<St1, St2>
where
    St1: Stream,
    St2: Stream<Item = St1::Item>,
{
    type Item = St1::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<St1::Item>> {
        let this = self.project();
        if !*this.poll_stream2_first {
            poll_inner(this.poll_stream2_first, this.stream1, this.stream2, cx)
        } else {
            poll_inner(this.poll_stream2_first, this.stream2, this.stream1, cx)
        }
    }
}

fn poll_inner<St1, St2>(
    poll_stream2_first: &mut bool,
    mut a: Pin<&mut Option<St1>>,
    mut b: Pin<&mut Option<St2>>,
    cx: &mut Context<'_>,
) -> Poll<Option<St1::Item>>
where
    St1: Stream,
    St2: Stream<Item = St1::Item>,
{
    // `a.as_mut().as_pin_mut()` to go from `&mut Pin<&mut Option<St1>>`
    // to `Option<Pin<&mut St1>>` without moving out of `a`, so we can later
    // do `a.set(None)` to drop the completed stream.
    let a_done = match a.as_mut().as_pin_mut().map(|a| a.poll_next(cx)) {
        Some(Poll::Ready(Some(item))) => {
            *poll_stream2_first = !*poll_stream2_first;
            return Poll::Ready(Some(item));
        }
        Some(Poll::Pending) => false,
        Some(Poll::Ready(None)) | None => true,
    };

    if a_done {
        a.set(None);
    }

    let b_done = match b.as_mut().as_pin_mut().map(|b| b.poll_next(cx)) {
        Some(Poll::Ready(Some(item))) => {
            return Poll::Ready(Some(item));
        }
        Some(Poll::Pending) => false,
        Some(Poll::Ready(None)) | None => true,
    };

    if b_done {
        b.set(None);
    }

    if a_done && b_done {
        Poll::Ready(None)
    } else {
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use futures::channel::mpsc::channel;
    use futures::stream;
    use futures::SinkExt;
    use futures::StreamExt;
    use tokio::time::timeout;
    use tokio::time::Duration;

    use super::*;

    #[tokio::test]
    async fn test_no_deadlock() -> Result<()> {
        let (sender, receiver) = channel(10);
        let producer = stream::iter(vec![1u8, 2, 3, 4, 5, 6, 7]).filter_map(move |num| {
            let mut sender = sender.clone();
            async move {
                if num == 4 {
                    match sender.send(num).await {
                        Ok(()) => None,
                        Err(_) => Some(0),
                    }
                } else {
                    Some(num)
                }
            }
        });
        let combined = select_drop(producer, receiver);
        let mut collected = timeout(Duration::from_secs(1), combined.collect::<Vec<u8>>())
            .await
            .expect(
                "stream timed out, select_drop didn't prevent deadlock or something went wrong",
            );
        collected.sort_unstable();
        assert_eq!(collected, vec![1u8, 2, 3, 4, 5, 6, 7]);
        Ok(())
    }
}
