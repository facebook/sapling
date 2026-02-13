/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use anyhow::Error;
use bytes::Bytes;
use futures::prelude::*;
use futures::ready;
use futures::stream::Stream;
use futures::task::Context;
use futures::task::Poll;
use pin_project::pin_project;

use super::content_meta::ContentMetaProvider;
use super::error_meta::ErrorMeta;
use super::error_meta::ErrorMetaProvider;
use super::stream::encode_stream;
use crate::content_encoding::ContentEncoding;

/// Thread-safe error capture. Stores first error and counts extras.
pub struct ErrorCapture<E> {
    first_error: Mutex<Option<E>>,
    extra_error_count: AtomicU64,
}

impl<E> ErrorCapture<E> {
    pub fn new() -> Self {
        Self {
            first_error: Mutex::new(None),
            extra_error_count: AtomicU64::new(0),
        }
    }

    pub fn capture(&self, e: E) {
        let mut guard = self.first_error.lock().expect("poisoned lock");
        if guard.is_none() {
            *guard = Some(e);
        } else {
            drop(guard);
            self.extra_error_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn report(&self, errors: &mut ErrorMeta<E>) {
        errors
            .errors
            .extend(self.first_error.lock().expect("poisoned lock").take());
        errors.extra_error_count += self.extra_error_count.load(Ordering::Relaxed);
    }
}

/// Poll a TryStream, filtering errors into an ErrorCapture. Returns Ok items only.
fn poll_and_capture_errors<S, E>(
    mut stream: Pin<&mut S>,
    ctx: &mut Context<'_>,
    capture: &ErrorCapture<E>,
) -> Poll<Option<S::Ok>>
where
    S: TryStream<Error = E>,
{
    loop {
        match ready!(stream.as_mut().try_poll_next(ctx)) {
            Some(Ok(item)) => return Poll::Ready(Some(item)),
            Some(Err(e)) => capture.capture(e),
            None => return Poll::Ready(None),
        }
    }
}

/// A stream that filters errors during polling, capturing them to shared state.
#[pin_project]
struct ErrorFilteringStream<S> {
    #[pin]
    stream: S,
    capture: Arc<ErrorCapture<Error>>,
}

impl<S> Stream for ErrorFilteringStream<S>
where
    S: TryStream<Ok = Bytes, Error = Error>,
{
    type Item = Result<Bytes, Error>;

    fn poll_next(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        poll_and_capture_errors(this.stream, ctx, this.capture).map(|opt| opt.map(Ok))
    }
}

pub trait ResponseTryStreamExt: TryStream {
    /// Filter out errors from a `TryStream` and capture the first one (then count further errors),
    /// transforming the `TryStream` into a `Stream<Item=Self::Ok>`.
    fn capture_first_err<E>(self) -> CaptureFirstErr<Self, E>
    where
        Self: Sized,
    {
        CaptureFirstErr::new(self)
    }

    /// Immediately end the `TryStream` upon encountering an error.
    ///
    /// The error will be passed to the given callback, and the stream will be
    /// fused to prevent the underlying `TryStream` from being polled again.
    fn end_on_err<E>(self) -> EndOnErr<Self, E>
    where
        Self: Sized,
    {
        EndOnErr::new(self)
    }
}

impl<S: TryStream + ?Sized> ResponseTryStreamExt for S {}

/// A stream that ignores errors raised by the internal stream and captures the first one.
#[pin_project]
pub struct CaptureFirstErr<S, E> {
    #[pin]
    stream: S,
    capture: ErrorCapture<E>,
}

impl<S, E> CaptureFirstErr<S, E> {
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            capture: ErrorCapture::new(),
        }
    }

    pub fn get_ref(&self) -> &S {
        &self.stream
    }
}

impl<S, E> Stream for CaptureFirstErr<S, E>
where
    S: TryStream<Error = E>,
{
    type Item = S::Ok;

    fn poll_next(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        poll_and_capture_errors(this.stream, ctx, this.capture)
    }
}

impl<S, E> ErrorMetaProvider<E> for CaptureFirstErr<S, E>
where
    S: TryStream<Error = E>,
{
    fn report_errors(self: Pin<&mut Self>, errors: &mut ErrorMeta<E>) {
        let this = self.project();
        this.capture.report(errors);
    }
}

impl<S> CaptureFirstErr<S, Error>
where
    S: TryStream<Ok = Bytes, Error = Error> + Send + 'static,
{
    /// Encode the stream with the specified Content-Encoding.
    ///
    /// Errors are filtered before compression to avoid tokio-util's ReaderStream
    /// fusing bug, but are still reported via ErrorMetaProvider.
    pub fn encode(self, encoding: ContentEncoding) -> EncodedCaptureFirstErr {
        let capture = Arc::new(ErrorCapture::new());

        // Filter errors before compression using ErrorFilteringStream
        let stream = ErrorFilteringStream {
            stream: self.stream,
            capture: capture.clone(),
        };

        let inner = encode_stream(stream, encoding, None).capture_first_err();

        EncodedCaptureFirstErr {
            inner: Box::pin(inner),
            encoding,
            capture,
        }
    }
}

/// A compressed stream that preserves error reporting from pre-compression errors.
#[pin_project]
pub struct EncodedCaptureFirstErr {
    #[pin]
    inner: Pin<Box<dyn Stream<Item = Bytes> + Send>>,
    encoding: ContentEncoding,
    capture: Arc<ErrorCapture<Error>>,
}

impl Stream for EncodedCaptureFirstErr {
    type Item = Bytes;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.project().inner.poll_next(cx)
    }
}

impl ErrorMetaProvider<Error> for EncodedCaptureFirstErr {
    fn report_errors(self: Pin<&mut Self>, errors: &mut ErrorMeta<Error>) {
        let this = self.project();
        this.capture.report(errors);
    }
}

impl ContentMetaProvider for EncodedCaptureFirstErr {
    fn content_encoding(&self) -> ContentEncoding {
        self.encoding
    }

    fn content_length(&self) -> Option<u64> {
        None
    }
}

#[pin_project]
pub struct EndOnErr<S, E> {
    #[pin]
    stream: S,
    errored: bool,
    error: Option<E>,
}

impl<S, E> EndOnErr<S, E> {
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            errored: false,
            error: None,
        }
    }

    pub fn get_ref(&self) -> &S {
        &self.stream
    }
}

impl<S, E> Stream for EndOnErr<S, E>
where
    S: TryStream<Error = E>,
{
    type Item = S::Ok;

    fn poll_next(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();

        // Fuse the stream once the error callback has fired.
        if *this.errored {
            return Poll::Ready(None);
        }

        match ready!(this.stream.try_poll_next(ctx)) {
            Some(Ok(item)) => Poll::Ready(Some(item)),
            Some(Err(e)) => {
                this.error.replace(e);
                *this.errored = true;
                Poll::Ready(None)
            }
            None => Poll::Ready(None),
        }
    }
}

impl<S, E> ErrorMetaProvider<E> for EndOnErr<S, E>
where
    S: TryStream<Error = E>,
{
    fn report_errors(self: Pin<&mut Self>, error_meta: &mut ErrorMeta<E>) {
        let this = self.project();
        error_meta.errors.extend(this.error.take());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_end_on_err() {
        let s = stream::iter(vec![
            Ok("hello"),
            Ok("world"),
            Err("error"),
            Ok("foo"),
            Err("bar"),
        ])
        .end_on_err();

        futures::pin_mut!(s);

        assert_eq!(s.next().await, Some("hello"));
        assert_eq!(s.next().await, Some("world"));
        assert_eq!(s.next().await, None);

        let mut errors = ErrorMeta::new();
        s.report_errors(&mut errors);
        assert_eq!(&errors.errors, &["error"]);
    }

    #[tokio::test]
    async fn test_capture_first_err() {
        let s = stream::iter(vec![
            Ok("hello"),
            Ok("world"),
            Err("error"),
            Ok("foo"),
            Err("bar"),
        ])
        .capture_first_err();

        futures::pin_mut!(s);

        assert_eq!(s.next().await, Some("hello"));
        assert_eq!(s.next().await, Some("world"));
        assert_eq!(s.next().await, Some("foo"));
        assert_eq!(s.next().await, None);

        let mut errors = ErrorMeta::new();
        s.report_errors(&mut errors);
        assert_eq!(&errors.errors, &["error"]);
        assert_eq!(errors.extra_error_count, 1);
    }
}
