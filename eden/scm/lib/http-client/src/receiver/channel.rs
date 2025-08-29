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

use crate::errors::Abort;
use crate::errors::HttpClientError;
use crate::header::Header;
use crate::receiver::Receiver;

type Headers = flume::Receiver<Header>;
type Done = oneshot::Receiver<Result<(), HttpClientError>>;

/// The receiving end of a `ChannelReceiver`.
pub struct ResponseStreams {
    pub headers_rx: Headers,
    pub body_rx: Pin<Box<dyn Stream<Item = Vec<u8>> + Send>>,
    pub done_rx: Done,
}

/// A `Receiver` that forwards all received data into channels.
pub struct ChannelReceiver {
    headers_tx: flume::Sender<Header>,
    body_tx: flume::Sender<Vec<u8>>,
    done_tx: Option<oneshot::Sender<Result<(), HttpClientError>>>,
    is_paused: bool,
}

impl ChannelReceiver {
    pub fn new(limit_buffer: bool) -> (Self, ResponseStreams) {
        let (headers_tx, headers_rx) = flume::unbounded();

        // Arbitrary queue limit. Big enough to keep the pipeline full, but small enough
        // to not use "all" the memory with potentially hundreds of concurrent requests.
        const BODY_CHUNK_QUEUE_SIZE: usize = 100;

        let (body_tx, body_rx) = if limit_buffer {
            flume::bounded(BODY_CHUNK_QUEUE_SIZE)
        } else {
            flume::unbounded()
        };

        let (done_tx, done_rx) = oneshot::channel();

        let senders = Self {
            headers_tx,
            body_tx,
            done_tx: Some(done_tx),
            is_paused: Default::default(),
        };

        let streams = ResponseStreams {
            headers_rx,
            body_rx: body_rx.into_stream().boxed(),
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
            Err(flume::TrySendError::Full(_)) => {
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

        !self.body_tx.is_full()
    }

    fn is_paused(&self) -> bool {
        self.is_paused
    }
}
