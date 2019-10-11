/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! Note: this code is from https://docs.rs/futures-util/0.2.0-beta/src/futures_util/stream/select_all.rs.html#23-25
//! with a couple of modifications to make it work with futures 0.1.*.
//! TODO(stash): When futures 0.2 will be supported, then we need to use upstream stream_all()
//!
//! An unbounded set of streams

use std::fmt::{self, Debug};

use futures::{Async, Poll, Stream};

use futures::stream::{FuturesUnordered, StreamFuture};

/// An unbounded set of streams
///
/// This "combinator" provides the ability to maintain a set of streams
/// and drive them all to completion.
///
/// Streams are pushed into this set and their realized values are
/// yielded as they become ready. Streams will only be polled when they
/// generate notifications. This allows to coordinate a large number of streams.
///
/// Note that you can create a ready-made `SelectAll` via the
/// `select_all` function in the `stream` module, or you can start with an
/// empty set with the `SelectAll::new` constructor.
#[must_use = "streams do nothing unless polled"]
pub struct SelectAll<S: Stream> {
    inner: FuturesUnordered<StreamFuture<S>>,
}

impl<T: Debug + Stream> Debug for SelectAll<T> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "SelectAll {{ ... }}")
    }
}

impl<S: Stream> SelectAll<S> {
    /// Constructs a new, empty `SelectAll`
    ///
    /// The returned `SelectAll` does not contain any streams and, in this
    /// state, `SelectAll::poll` will return `Ok(Async::Ready(None))`.
    pub fn new() -> SelectAll<S> {
        SelectAll {
            inner: FuturesUnordered::new(),
        }
    }

    /// Returns the number of streams contained in the set.
    ///
    /// This represents the total number of in-flight streams.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns `true` if the set contains no streams
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Push a stream into the set.
    ///
    /// This function submits the given stream to the set for managing. This
    /// function will not call `poll` on the submitted stream. The caller must
    /// ensure that `SelectAll::poll` is called in order to receive task
    /// notifications.
    pub fn push(&mut self, stream: S) {
        self.inner.push(stream.into_future());
    }
}

impl<S: Stream> Stream for SelectAll<S> {
    type Item = S::Item;
    type Error = S::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            match self.inner.poll().map_err(|(err, _)| err)? {
                Async::NotReady => {
                    return Ok(Async::NotReady);
                }
                Async::Ready(Some((Some(item), remaining))) => {
                    self.push(remaining);
                    return Ok(Async::Ready(Some(item)));
                }
                Async::Ready(Some((None, _))) => {
                    // One of the internal streams was exhausted. Nothing needs to be done with
                    // this internal stream anymore.
                    continue;
                }
                Async::Ready(None) => {
                    return Ok(Async::Ready(None));
                }
            }
        }
    }
}

/// Convert a list of streams into a `Stream` of results from the streams.
///
/// This essentially takes a list of streams (e.g. a vector, an iterator, etc.)
/// and bundles them together into a single stream.
/// The stream will yield items as they become available on the underlying
/// streams internally, in the order they become available.
///
/// Note that the returned set can also be used to dynamically push more
/// futures into the set as they become available.
pub fn select_all<I>(streams: I) -> SelectAll<I::Item>
where
    I: IntoIterator,
    I::Item: Stream,
{
    let mut set = SelectAll::new();

    for stream in streams {
        set.push(stream);
    }

    return set;
}

#[cfg(test)]
mod tests {
    use super::*;

    use async_unit::tokio_unit_test;
    use futures::executor::spawn;
    use futures::stream::iter_ok;

    #[test]
    fn select_all_many_streams() {
        tokio_unit_test(|| {
            let streams: Vec<_> = (1..5)
                .into_iter()
                .map(|i| iter_ok::<_, ()>((0..i).into_iter()))
                .collect();
            let result = spawn(select_all(streams).collect()).wait_future().unwrap();
            assert_eq!(result.len(), 10);
        })
    }
}
