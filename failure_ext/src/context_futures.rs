/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use failure::{Context, Error, Fail};
use futures::{Future, Poll};
use std::fmt::Display;

// "Context" support for futures where the error is failure::Error.
pub trait FutureFailureErrorExt: Future + Sized {
    fn context<D>(self, context: D) -> ContextErrorFut<Self, D>
    where
        D: Display + Send + Sync + 'static;

    fn with_context<D, F>(self, f: F) -> WithContextErrorFut<Self, F>
    where
        D: Display + Send + Sync + 'static,
        F: FnOnce() -> D;
}

impl<F> FutureFailureErrorExt for F
where
    F: Future<Error = Error> + Sized,
{
    fn context<D>(self, displayable: D) -> ContextErrorFut<Self, D>
    where
        D: Display + Send + Sync + 'static,
    {
        ContextErrorFut::new(self, displayable)
    }

    fn with_context<D, O>(self, f: O) -> WithContextErrorFut<Self, O>
    where
        D: Display + Send + Sync + 'static,
        O: FnOnce() -> D,
    {
        WithContextErrorFut::new(self, f)
    }
}

pub struct WithContextErrorFut<A, F> {
    inner: A,
    displayable: Option<F>,
}

impl<A, F, D> WithContextErrorFut<A, F>
where
    A: Future<Error = Error>,
    D: Display + Send + Sync + 'static,
    F: FnOnce() -> D,
{
    pub fn new(future: A, displayable: F) -> Self {
        Self {
            inner: future,
            displayable: Some(displayable),
        }
    }
}

impl<A, F, D> Future for WithContextErrorFut<A, F>
where
    A: Future<Error = Error>,
    D: Display + Send + Sync + 'static,
    F: FnOnce() -> D,
{
    type Item = A::Item;
    type Error = Context<D>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.inner.poll() {
            Err(err) => {
                let f = self
                    .displayable
                    .take()
                    .expect("poll called after future completion");

                let context = f();
                Err(err.context(context))
            }
            Ok(item) => Ok(item),
        }
    }
}

pub struct ContextErrorFut<A, D> {
    inner: A,
    displayable: Option<D>,
}

impl<A, D> ContextErrorFut<A, D>
where
    A: Future<Error = Error>,
    D: Display + Send + Sync + 'static,
{
    pub fn new(future: A, displayable: D) -> Self {
        Self {
            inner: future,
            displayable: Some(displayable),
        }
    }
}

impl<A, D> Future for ContextErrorFut<A, D>
where
    A: Future<Error = Error>,
    D: Display + Send + Sync + 'static,
{
    type Item = A::Item;
    type Error = Context<D>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.inner.poll() {
            Err(err) => Err(err.context(
                self.displayable
                    .take()
                    .expect("poll called after future completion"),
            )),
            Ok(item) => Ok(item),
        }
    }
}

// "Context" support for futures where the error is an implementation of failure::Fail.
pub trait FutureFailureExt: Future + Sized {
    fn context<D>(self, context: D) -> ContextFut<Self, D>
    where
        D: Display + Send + Sync + 'static;

    fn with_context<D, F>(self, f: F) -> WithContextFut<Self, F>
    where
        D: Display + Send + Sync + 'static,
        F: FnOnce() -> D;
}

impl<F> FutureFailureExt for F
where
    F: Future + Sized,
    F::Error: Fail,
{
    fn context<D>(self, displayable: D) -> ContextFut<Self, D>
    where
        D: Display + Send + Sync + 'static,
    {
        ContextFut::new(self, displayable)
    }

    fn with_context<D, O>(self, f: O) -> WithContextFut<Self, O>
    where
        D: Display + Send + Sync + 'static,
        O: FnOnce() -> D,
    {
        WithContextFut::new(self, f)
    }
}

pub struct ContextFut<A, D> {
    inner: A,
    displayable: Option<D>,
}

impl<A, D> ContextFut<A, D>
where
    A: Future,
    A::Error: Fail,
    D: Display + Send + Sync + 'static,
{
    pub fn new(future: A, displayable: D) -> Self {
        Self {
            inner: future,
            displayable: Some(displayable),
        }
    }
}

impl<A, D> Future for ContextFut<A, D>
where
    A: Future,
    A::Error: Fail,
    D: Display + Send + Sync + 'static,
{
    type Item = A::Item;
    type Error = Context<D>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.inner.poll() {
            Err(err) => Err(err.context(
                self.displayable
                    .take()
                    .expect("poll called after future completion"),
            )),
            Ok(item) => Ok(item),
        }
    }
}

pub struct WithContextFut<A, F> {
    inner: A,
    displayable: Option<F>,
}

impl<A, D, F> WithContextFut<A, F>
where
    A: Future,
    A::Error: Fail,
    D: Display + Send + Sync + 'static,
    F: FnOnce() -> D,
{
    pub fn new(future: A, displayable: F) -> Self {
        Self {
            inner: future,
            displayable: Some(displayable),
        }
    }
}

impl<A, D, F> Future for WithContextFut<A, F>
where
    A: Future,
    A::Error: Fail,
    D: Display + Send + Sync + 'static,
    F: FnOnce() -> D,
{
    type Item = A::Item;
    type Error = Context<D>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.inner.poll() {
            Err(err) => {
                let f = self
                    .displayable
                    .take()
                    .expect("poll called after future completion");

                let context = f();
                Err(err.context(context))
            }
            Ok(item) => Ok(item),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use futures::future::err;

    #[test]
    #[should_panic]
    fn poll_after_completion_fail() {
        let err = err::<(), _>(format_err!("foo").context("bar"));
        let mut err = err.context("baz");
        let _ = err.poll();
        let _ = err.poll();
    }

    #[test]
    #[should_panic]
    fn poll_after_completion_fail_with_context() {
        let err = err::<(), _>(format_err!("foo").context("bar"));
        let mut err = err.with_context(|| "baz");
        let _ = err.poll();
        let _ = err.poll();
    }

    #[test]
    #[should_panic]
    fn poll_after_completion_error() {
        let err = err::<(), _>(format_err!("foo"));
        let mut err = err.context("baz");
        let _ = err.poll();
        let _ = err.poll();
    }

    #[test]
    #[should_panic]
    fn poll_after_completion_error_with_context() {
        let err = err::<(), _>(format_err!("foo"));
        let mut err = err.with_context(|| "baz");
        let _ = err.poll();
        let _ = err.poll();
    }
}
