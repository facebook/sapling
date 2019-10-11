/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use futures::{Async, Future, Poll, Stream};
use std::default::Default;
use std::mem;

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct CollectTo<S, C> {
    stream: S,
    collection: C,
}

impl<S: Stream, C> CollectTo<S, C>
where
    C: Default + Extend<S::Item>,
{
    fn finish(&mut self) -> C {
        mem::replace(&mut self.collection, Default::default())
    }

    pub fn new(stream: S) -> CollectTo<S, C> {
        CollectTo {
            stream,
            collection: Default::default(),
        }
    }
}

impl<S, C> Future for CollectTo<S, C>
where
    S: Stream,
    C: Default + Extend<S::Item>,
{
    type Item = C;
    type Error = S::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match self.stream.poll() {
                Ok(Async::Ready(Some(v))) => self.collection.extend(Some(v)),
                Ok(Async::Ready(None)) => return Ok(Async::Ready(self.finish())),
                Ok(Async::NotReady) => return Ok(Async::NotReady),
                Err(e) => {
                    self.finish();
                    return Err(e);
                }
            }
        }
    }
}
