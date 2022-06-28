/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use changeset_fetcher::ArcChangesetFetcher;
use cloned::cloned;
use context::CoreContext;
use futures::FutureExt;
use futures::TryFutureExt;
use futures_ext::BoxStream;
use futures_ext::StreamExt;
use futures_old::future::Future;
use futures_old::stream::Stream;
use mononoke_types::ChangesetId;
use mononoke_types::Generation;

use crate::errors::*;
use crate::BonsaiNodeStream;
use anyhow::Error;

use futures_old::Async;
use futures_old::Poll;

type GenericStream<T> = BoxStream<(T, Generation), Error>;
pub type BonsaiInputStream = GenericStream<ChangesetId>;

pub fn add_generations_by_bonsai(
    ctx: CoreContext,
    stream: BonsaiNodeStream,
    changeset_fetcher: ArcChangesetFetcher,
) -> BonsaiInputStream {
    stream
        .map(move |changesetid| {
            cloned!(ctx, changeset_fetcher);
            async move {
                changeset_fetcher
                    .get_generation_number(ctx, changesetid)
                    .await
            }
            .boxed()
            .compat()
            .map(move |gen_id| (changesetid, gen_id))
            .map_err(|err| err.context(ErrorKind::GenerationFetchFailed))
        })
        .buffered(100)
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
mod test_utils {
    use super::*;

    use anyhow::bail;
    use futures_old::task;
    use mercurial_types::HgNodeHash;
    use std::marker::PhantomData;

    pub struct NotReadyEmptyStream<T> {
        pub poll_count: usize,
        __phantom: PhantomData<T>,
    }

    impl<T> NotReadyEmptyStream<T> {
        pub fn new(poll_count: usize) -> Self {
            Self {
                poll_count,
                __phantom: PhantomData,
            }
        }
    }

    impl<T> Stream for NotReadyEmptyStream<T> {
        type Item = T;
        type Error = Error;

        fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            if self.poll_count == 0 {
                Ok(Async::Ready(None))
            } else {
                self.poll_count -= 1;
                task::current().notify();
                Ok(Async::NotReady)
            }
        }
    }

    pub struct RepoErrorStream<T> {
        pub item: T,
    }

    impl Stream for RepoErrorStream<HgNodeHash> {
        type Item = HgNodeHash;
        type Error = Error;

        fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            bail!(ErrorKind::RepoNodeError(self.item));
        }
    }

    impl Stream for RepoErrorStream<ChangesetId> {
        type Item = ChangesetId;
        type Error = Error;

        fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            bail!(ErrorKind::RepoChangesetError(self.item));
        }
    }
}

#[cfg(test)]
pub use test_utils::NotReadyEmptyStream;
#[cfg(test)]
pub use test_utils::RepoErrorStream;
