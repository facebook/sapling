/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use futures::channel::mpsc;
use futures::channel::oneshot;

use crate::errors::Abort;
use crate::errors::HttpClientError;
use crate::header::Header;
use crate::receiver::Receiver;

type Headers = mpsc::UnboundedReceiver<Header>;
type Chunks = mpsc::UnboundedReceiver<Vec<u8>>;
type Done = oneshot::Receiver<Result<(), HttpClientError>>;

/// The receiving end of a `ChannelReceiver`.
pub struct ResponseStreams {
    pub headers_rx: Headers,
    pub body_rx: Chunks,
    pub done_rx: Done,
}

/// A `Receiver` that forwards all received data into channels.
pub struct ChannelReceiver {
    headers_tx: mpsc::UnboundedSender<Header>,
    body_tx: mpsc::UnboundedSender<Vec<u8>>,
    done_tx: oneshot::Sender<Result<(), HttpClientError>>,
}

impl ChannelReceiver {
    pub fn new() -> (Self, ResponseStreams) {
        let (headers_tx, headers_rx) = mpsc::unbounded();
        let (body_tx, body_rx) = mpsc::unbounded();
        let (done_tx, done_rx) = oneshot::channel();

        let senders = Self {
            headers_tx,
            body_tx,
            done_tx,
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
    fn chunk(&mut self, chunk: Vec<u8>) -> Result<()> {
        self.body_tx.unbounded_send(chunk).map_err(|e| e.into())
    }

    fn header(&mut self, header: Header) -> Result<()> {
        self.headers_tx.unbounded_send(header).map_err(|e| e.into())
    }

    fn done(self, res: Result<(), HttpClientError>) -> Result<(), Abort> {
        let _ = self.done_tx.send(res);
        Ok(())
    }
}
