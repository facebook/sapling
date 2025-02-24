/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::pin::Pin;
use std::task::Poll;

use anyhow::Result;
use futures::channel::mpsc;
use futures::channel::oneshot;
use futures::Stream;
use futures::StreamExt;

use crate::errors::Abort;
use crate::errors::HttpClientError;
use crate::header::Header;
use crate::receiver::Receiver;

type Headers = mpsc::UnboundedReceiver<Header>;
type Done = oneshot::Receiver<Result<(), HttpClientError>>;

/// The receiving end of a `ChannelReceiver`.
pub struct ResponseStreams {
    pub headers_rx: Headers,
    pub body_rx: Pin<Box<dyn Stream<Item = Vec<u8>> + Send>>,
    pub done_rx: Done,
}

/// A `Receiver` that forwards all received data into channels.
pub struct ChannelReceiver {
    headers_tx: mpsc::UnboundedSender<Header>,
    body_tx: BodySender,
    done_tx: Option<oneshot::Sender<Result<(), HttpClientError>>>,
    is_paused: bool,
}

enum BodySender {
    Limited(mpsc::Sender<Vec<u8>>),
    Unlimited(mpsc::UnboundedSender<Vec<u8>>),
}

impl ChannelReceiver {
    pub fn new(limit_buffer: bool) -> (Self, ResponseStreams) {
        let (headers_tx, headers_rx) = mpsc::unbounded();

        // Arbitrary queue limit. Big enough to keep the pipeline full, but small enough
        // to not use "all" the memory with potentially hundreds of concurrent requests.
        const BODY_CHUNK_QUEUE_SIZE: usize = 1000;

        let (body_tx, body_rx) = if limit_buffer {
            let (body_tx, body_rx) = mpsc::channel(BODY_CHUNK_QUEUE_SIZE);
            (BodySender::Limited(body_tx), body_rx.boxed())
        } else {
            let (body_tx, body_rx) = mpsc::unbounded();
            (BodySender::Unlimited(body_tx), body_rx.boxed())
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
            body_rx,
            done_rx,
        };

        (senders, streams)
    }
}

impl Receiver for ChannelReceiver {
    fn chunk(&mut self, chunk: Vec<u8>) -> Result<bool> {
        match &mut self.body_tx {
            BodySender::Limited(sender) => {
                match sender.try_send(chunk) {
                    Ok(()) => {
                        // we enqueued something, definitely not paused
                        self.is_paused = false;

                        Ok(false)
                    }
                    // Queue is full - tell curl to pause the transfer.
                    Err(err) if err.is_full() => {
                        self.is_paused = true;

                        Ok(true)
                    }
                    Err(err) => Err(err.into()),
                }
            }
            BodySender::Unlimited(unbounded_sender) => {
                unbounded_sender
                    .unbounded_send(chunk)
                    .map_err(anyhow::Error::from)?;
                Ok(false)
            }
        }
    }

    fn header(&mut self, header: Header) -> Result<()> {
        self.headers_tx.unbounded_send(header).map_err(|e| e.into())
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

        if let BodySender::Limited(ref mut sender) = self.body_tx {
            // Use no-op waker since we don't have any tasks blocked trying to insert into
            // queue (i.e. no one needs to be woken up).
            let waker = futures::task::noop_waker_ref();
            let mut cx = futures::task::Context::from_waker(waker);
            match sender.poll_ready(&mut cx) {
                // queue has space - unpause
                Poll::Ready(Ok(_)) => true,
                // queue has been dropped - unpause so the error gets propagated via chunk()
                Poll::Ready(Err(_)) => true,
                // maybe no space - stay paused
                Poll::Pending => false,
            }
        } else {
            unreachable!("unlimited sender is paused?")
        }
    }
}
