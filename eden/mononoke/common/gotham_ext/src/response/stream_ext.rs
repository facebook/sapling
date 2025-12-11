/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::pin::Pin;

use futures::prelude::*;
use futures::ready;
use futures::stream::Stream;
use futures::task::Context;
use futures::task::Poll;
use pin_project::pin_project;

use super::error_meta::ErrorMeta;
use super::error_meta::ErrorMetaProvider;

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
    /// First error this stream encountered.
    first_error: Option<E>,
    /// Count of errors that were observed but not captured.
    extra_error_count: u64,
}

impl<S, E> CaptureFirstErr<S, E> {
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            first_error: None,
            extra_error_count: 0,
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
        let mut this = self.project();

        loop {
            match ready!(this.stream.as_mut().try_poll_next(ctx)) {
                Some(Ok(item)) => return Poll::Ready(Some(item)),
                Some(Err(e)) => {
                    if this.first_error.is_some() {
                        *this.extra_error_count += 1;
                    } else {
                        this.first_error.replace(e);
                    }
                }
                None => return Poll::Ready(None),
            }
        }
    }
}

impl<S, E> ErrorMetaProvider<E> for CaptureFirstErr<S, E>
where
    S: TryStream<Error = E>,
{
    fn report_errors(self: Pin<&mut Self>, errors: &mut ErrorMeta<E>) {
        let this = self.project();
        errors.errors.extend(this.first_error.take());
        errors.extra_error_count += *this.extra_error_count;
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
