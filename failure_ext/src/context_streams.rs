// Copyright 2004-present Facebook. All Rights Reserved.

use failure::{Context, Error, Fail};
use futures::{Poll, Stream};
use std::fmt::Display;

// "Context" support for streams where the error is an implementation of failure::Fail.
pub trait StreamFailureExt: Stream + Sized {
    fn context<D>(self, context: D) -> ContextStream<Self, D>
    where
        D: Display + Clone + Send + Sync + 'static;

    fn with_context<D, F>(self, f: F) -> WithContextStream<Self, F>
    where
        D: Display + Clone + Send + Sync + 'static,
        F: FnMut(&dyn Fail) -> D;
}

impl<S> StreamFailureExt for S
where
    S: Stream + Sized,
    S::Error: Fail,
{
    fn context<D>(self, displayable: D) -> ContextStream<Self, D>
    where
        D: Display + Clone + Send + Sync + 'static,
    {
        ContextStream::new(self, displayable)
    }

    fn with_context<D, F>(self, f: F) -> WithContextStream<Self, F>
    where
        D: Display + Clone + Send + Sync + 'static,
        F: FnMut(&dyn Fail) -> D,
    {
        WithContextStream::new(self, f)
    }
}

pub struct ContextStream<A, D> {
    inner: A,
    displayable: D,
}

impl<A, D> ContextStream<A, D> {
    fn new(stream: A, displayable: D) -> Self {
        Self {
            inner: stream,
            displayable,
        }
    }
}

impl<A, D> Stream for ContextStream<A, D>
where
    A: Stream,
    A::Error: Fail,
    D: Display + Clone + Send + Sync + 'static,
{
    type Item = A::Item;
    type Error = Context<D>;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.inner.poll() {
            Err(err) => Err(err.context(self.displayable.clone())),
            Ok(item) => Ok(item),
        }
    }
}

pub struct WithContextStream<A, F> {
    inner: A,
    displayable: F,
}

impl<A, F> WithContextStream<A, F> {
    fn new(stream: A, displayable: F) -> Self {
        Self {
            inner: stream,
            displayable,
        }
    }
}

impl<A, F, D> Stream for WithContextStream<A, F>
where
    A: Stream,
    A::Error: Fail,
    D: Display + Clone + Send + Sync + 'static,
    F: FnMut(&dyn Fail) -> D,
{
    type Item = A::Item;
    type Error = Context<D>;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.inner.poll() {
            Err(err) => {
                let context = (&mut self.displayable)(&err);
                Err(err.context(context))
            }
            Ok(item) => Ok(item),
        }
    }
}

// "Context" support for streams where the error is an implementation of failure::Error.
pub trait StreamFailureErrorExt: Stream + Sized {
    fn context<D>(self, context: D) -> ContextErrorStream<Self, D>
    where
        D: Display + Clone + Send + Sync + 'static;

    fn with_context<D, F>(self, f: F) -> WithContextErrorStream<Self, F>
    where
        D: Display + Clone + Send + Sync + 'static,
        F: FnMut(&Error) -> D;
}

impl<S> StreamFailureErrorExt for S
where
    S: Stream<Error = Error> + Sized,
{
    fn context<D>(self, displayable: D) -> ContextErrorStream<Self, D>
    where
        D: Display + Clone + Send + Sync + 'static,
    {
        ContextErrorStream::new(self, displayable)
    }

    fn with_context<D, F>(self, f: F) -> WithContextErrorStream<Self, F>
    where
        D: Display + Clone + Send + Sync + 'static,
        F: FnMut(&Error) -> D,
    {
        WithContextErrorStream::new(self, f)
    }
}

pub struct ContextErrorStream<A, D> {
    inner: A,
    displayable: D,
}

impl<A, D> ContextErrorStream<A, D> {
    fn new(stream: A, displayable: D) -> Self {
        Self {
            inner: stream,
            displayable,
        }
    }
}

impl<A, D> Stream for ContextErrorStream<A, D>
where
    A: Stream<Error = Error>,
    D: Display + Clone + Send + Sync + 'static,
{
    type Item = A::Item;
    type Error = Context<D>;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.inner.poll() {
            Err(err) => Err(err.context(self.displayable.clone())),
            Ok(item) => Ok(item),
        }
    }
}

pub struct WithContextErrorStream<A, F> {
    inner: A,
    displayable: F,
}

impl<A, F> WithContextErrorStream<A, F> {
    fn new(stream: A, displayable: F) -> Self {
        Self {
            inner: stream,
            displayable,
        }
    }
}

impl<A, F, D> Stream for WithContextErrorStream<A, F>
where
    A: Stream<Error = Error>,
    D: Display + Clone + Send + Sync + 'static,
    F: FnMut(&Error) -> D,
{
    type Item = A::Item;
    type Error = Context<D>;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.inner.poll() {
            Err(err) => {
                let context = (&mut self.displayable)(&err);
                Err(err.context(context))
            }
            Ok(item) => Ok(item),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use futures::stream::iter_result;

    #[test]
    fn stream_poll_after_completion_fail() {
        let stream = iter_result(vec![
            Ok(17),
            Err(format_err!("foo").context("bar")),
            Err(format_err!("baz").context("wiggle")),
        ]);
        let mut stream = stream.context("foo");
        let _ = stream.poll();
        let _ = stream.poll();
        let _ = stream.poll();
    }

    #[test]
    fn stream_poll_after_completion_fail_with_context() {
        let stream = iter_result(vec![
            Ok(17),
            Err(format_err!("foo").context("bar")),
            Err(format_err!("baz").context("wiggle")),
        ]);
        let mut stream = stream.with_context(move |_| "foo");
        let _ = stream.poll();
        let _ = stream.poll();
        let _ = stream.poll();
    }

    #[test]
    fn stream_poll_after_completion_error() {
        let stream = iter_result(vec![
            Ok(17),
            Err(format_err!("bar")),
            Err(format_err!("baz")),
        ]);
        let mut stream = stream.context("foo");
        let _ = stream.poll();
        let _ = stream.poll();
        let _ = stream.poll();
    }

    #[test]
    fn stream_poll_after_completion_error_with_context() {
        let stream = iter_result(vec![
            Ok(17),
            Err(format_err!("bar")),
            Err(format_err!("baz")),
        ]);
        let mut stream = stream.with_context(move |_| "foo");
        let _ = stream.poll();
        let _ = stream.poll();
        let _ = stream.poll();
    }
}
