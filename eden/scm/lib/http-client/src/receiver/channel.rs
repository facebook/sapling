/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::pin::Pin;

use anyhow::Result;
use futures::Stream;
use futures::StreamExt;
use futures::channel::oneshot;
use futures::stream;

use crate::errors::Abort;
use crate::errors::HttpClientError;
use crate::header::Header;
use crate::receiver::Receiver;

type Headers = flume::Receiver<Header>;
type Done = oneshot::Receiver<Result<(), HttpClientError>>;
pub(crate) const DEFAULT_RESPONSE_BUFFER_LENGTH: usize = 16;

/// The receiving end of a `ChannelReceiver`.
pub struct ResponseStreams {
    pub headers_rx: Headers,
    pub body_rx: Pin<Box<dyn Stream<Item = Vec<u8>> + Send>>,
    pub done_rx: Done,
}

/// A `Receiver` that forwards all received data into channels.
pub struct ChannelReceiver {
    headers_tx: flume::Sender<Header>,
    body_tx: crossfire::MTx<crossfire::mpsc::Array<Vec<u8>>>,
    done_tx: Option<oneshot::Sender<Result<(), HttpClientError>>>,
    is_paused: bool,
}

impl ChannelReceiver {
    pub fn new(response_buffer_length: usize) -> (Self, ResponseStreams) {
        let (headers_tx, headers_rx) = flume::unbounded();

        let (body_tx, body_rx) =
            crossfire::mpsc::bounded_blocking_async(response_buffer_length.max(1));
        let body_rx = stream::unfold(body_rx, |body_rx| async move {
            body_rx.recv().await.ok().map(|chunk| (chunk, body_rx))
        })
        .boxed();

        let (done_tx, done_rx) = oneshot::channel();

        let senders = Self {
            headers_tx,
            body_tx,
            done_tx: Some(done_tx),
            is_paused: Default::default(),
        };

        let streams = ResponseStreams {
            headers_rx,
            body_rx,
            done_rx,
        };

        (senders, streams)
    }
}

impl Receiver for ChannelReceiver {
    fn chunk(&mut self, chunk: Vec<u8>) -> Result<bool> {
        match self.body_tx.try_send(chunk) {
            Ok(()) => {
                // we enqueued something, definitely not paused
                self.is_paused = false;
                Ok(false)
            }
            Err(crossfire::TrySendError::Full(_)) => {
                // Queue is full - tell curl to pause the transfer.
                self.is_paused = true;
                Ok(true)
            }
            Err(err) => Err(err.into()),
        }
    }

    fn header(&mut self, header: Header) -> Result<()> {
        self.headers_tx.send(header).map_err(|e| e.into())
    }

    fn done(&mut self, res: Result<(), HttpClientError>) -> Result<(), Abort> {
        if let Some(done_tx) = self.done_tx.take() {
            let _ = done_tx.send(res);
        }
        Ok(())
    }

    fn needs_unpause(&mut self) -> bool {
        if !self.is_paused {
            return false;
        }

        self.body_tx.is_disconnected() || !self.body_tx.is_full()
    }

    fn is_paused(&self) -> bool {
        self.is_paused
    }
}

#[cfg(test)]
mod tests {
    use futures::executor::block_on;

    use super::*;

    #[test]
    fn bounded_body_queue_pauses_and_resumes() -> Result<()> {
        let (mut receiver, mut streams) = ChannelReceiver::new(1);

        assert!(!receiver.chunk(vec![1])?);
        assert!(receiver.chunk(vec![2])?);
        assert!(receiver.is_paused());
        assert!(!receiver.needs_unpause());

        assert_eq!(block_on(streams.body_rx.next()), Some(vec![1]));
        assert!(receiver.needs_unpause());

        assert!(!receiver.chunk(vec![2])?);
        assert_eq!(block_on(streams.body_rx.next()), Some(vec![2]));
        Ok(())
    }

    #[test]
    fn dropped_body_receiver_unpauses_full_channel() -> Result<()> {
        let (mut receiver, streams) = ChannelReceiver::new(1);

        assert!(!receiver.chunk(vec![1])?);
        assert!(receiver.chunk(vec![2])?);
        drop(streams.body_rx);

        assert!(receiver.needs_unpause());
        assert!(receiver.chunk(vec![2]).is_err());
        Ok(())
    }
}
