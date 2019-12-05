/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use changeset_fetcher::ChangesetFetcher;
use context::CoreContext;
#[cfg(test)]
use failure_ext::bail;
use futures::future::Future;
use futures::stream::Stream;
use futures_ext::{BoxStream, StreamExt};
#[cfg(test)]
use mercurial_types::HgNodeHash;
use mononoke_types::{ChangesetId, Generation};
#[cfg(test)]
use std::marker::PhantomData;
use std::sync::Arc;

use crate::errors::*;
use crate::BonsaiNodeStream;
use failure_ext::Error;

use futures::{Async, Poll};

type GenericStream<T> = BoxStream<(T, Generation), Error>;
pub type BonsaiInputStream = GenericStream<ChangesetId>;

pub fn add_generations_by_bonsai(
    ctx: CoreContext,
    stream: BonsaiNodeStream,
    changeset_fetcher: Arc<dyn ChangesetFetcher>,
) -> BonsaiInputStream {
    stream
        .and_then(move |changesetid| {
            changeset_fetcher
                .get_generation_number(ctx.clone(), changesetid)
                .map(move |gen_id| (changesetid, gen_id))
                .map_err(|err| err.context(ErrorKind::GenerationFetchFailed).into())
        })
        .boxify()
}

pub fn all_inputs_ready<T>(
    inputs: &Vec<(GenericStream<T>, Poll<Option<(T, Generation)>, Error>)>,
) -> bool {
    inputs
        .iter()
        .map(|&(_, ref state)| match state {
            &Err(_) => false,
            &Ok(ref p) => p.is_ready(),
        })
        .all(|ready| ready)
}

pub fn poll_all_inputs<T>(
    inputs: &mut Vec<(GenericStream<T>, Poll<Option<(T, Generation)>, Error>)>,
) {
    for &mut (ref mut input, ref mut state) in inputs.iter_mut() {
        if let Ok(Async::NotReady) = *state {
            *state = input.poll();
        }
    }
}

#[cfg(test)]
pub struct NotReadyEmptyStream<T> {
    pub poll_count: usize,
    __phantom: PhantomData<T>,
}

#[cfg(test)]
impl<T> NotReadyEmptyStream<T> {
    pub fn new(poll_count: usize) -> Self {
        Self {
            poll_count,
            __phantom: PhantomData,
        }
    }
}

#[cfg(test)]
impl<T> Stream for NotReadyEmptyStream<T> {
    type Item = T;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if self.poll_count == 0 {
            Ok(Async::Ready(None))
        } else {
            self.poll_count -= 1;
            Ok(Async::NotReady)
        }
    }
}

#[cfg(test)]
pub struct RepoErrorStream<T> {
    pub item: T,
}

#[cfg(test)]
impl Stream for RepoErrorStream<HgNodeHash> {
    type Item = HgNodeHash;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        bail!(ErrorKind::RepoNodeError(self.item));
    }
}

#[cfg(test)]
impl Stream for RepoErrorStream<ChangesetId> {
    type Item = ChangesetId;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        bail!(ErrorKind::RepoChangesetError(self.item));
    }
}
