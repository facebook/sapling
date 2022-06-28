/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use chrono::DateTime;
use chrono::Utc;
use futures::sink::Sink;
use futures::task::Context;
use futures::task::Poll;
use pin_project::pin_project;
use sshrelay::IoStream;
use sshrelay::SshMsg;
use std::pin::Pin;

#[pin_project]
pub struct WireprotoSink<T> {
    #[pin]
    inner: T,
    pub data: WireprotoSinkData,
}

impl<T> WireprotoSink<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            data: WireprotoSinkData::new(),
        }
    }
}

impl<T> Sink<SshMsg> for WireprotoSink<T>
where
    T: Sink<SshMsg>,
{
    type Error = <T as Sink<SshMsg>>::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.project();
        let ret = this.inner.poll_ready(cx);
        this.data.peek_io(&ret);
        ret
    }

    fn start_send(self: Pin<&mut Self>, item: SshMsg) -> Result<(), Self::Error> {
        let this = self.project();
        this.data.peek_message(&item);
        this.inner.start_send(item)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.project();
        let ret = this.inner.poll_flush(cx);
        this.data.peek_io(&ret);
        this.data.peek_flush(&ret);
        ret
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.project();
        let ret = this.inner.poll_close(cx);
        this.data.peek_io(&ret);
        ret
    }
}

pub struct WireprotoSinkData {
    pub last_successful_flush: Option<DateTime<Utc>>,
    pub last_successful_io: Option<DateTime<Utc>>,
    pub last_failed_io: Option<DateTime<Utc>>,
    pub stdout: ChannelData,
    pub stderr: ChannelData,
}

impl WireprotoSinkData {
    fn new() -> Self {
        Self {
            last_successful_flush: None,
            last_successful_io: None,
            last_failed_io: None,
            stdout: ChannelData::default(),
            stderr: ChannelData::default(),
        }
    }

    fn peek_message(&mut self, item: &SshMsg) {
        match item.stream_ref() {
            IoStream::Stdout => self.stdout.peek(item.as_ref()),
            IoStream::Stderr => self.stderr.peek(item.as_ref()),
            IoStream::Stdin => {}
        }
    }

    fn peek_io<E>(&mut self, res: &Poll<Result<(), E>>) {
        match res {
            Poll::Pending => {}
            Poll::Ready(Ok(())) => {
                self.last_successful_io = Some(Utc::now());
            }
            Poll::Ready(Err(..)) => {
                self.last_failed_io = Some(Utc::now());
            }
        }
    }

    fn peek_flush<E>(&mut self, res: &Poll<Result<(), E>>) {
        match res {
            Poll::Pending => {}
            Poll::Ready(Ok(())) => {
                self.last_successful_flush = Some(Utc::now());
            }
            Poll::Ready(Err(..)) => {
                // No need for this it's already tracked in peek_io.
            }
        }
    }
}

#[derive(Default)]
pub struct ChannelData {
    pub messages: u64,
    pub bytes: u64,
}

impl ChannelData {
    pub fn peek(&mut self, data: &[u8]) {
        let len: u64 = data
            .len()
            .try_into()
            .expect("The length of a buffer that exists will fit in a u64");

        self.messages += 1;
        self.bytes += len;
    }
}
