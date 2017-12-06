// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashSet;
use std::mem;
use std::sync::Arc;

use futures::{Async, Poll};
use futures::future::Future;
use futures::stream::{self, Stream};
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};

use bookmarks::Bookmarks;
use heads::Heads;
use mercurial_types::{Changeset, Manifest, NodeHash, Repo};
use storage_types::Version;

use BlobChangeset;
use BlobManifest;
use BlobState;
use errors::*;
use file::fetch_blob_from_blobstore;

pub struct BlobRepo<State> {
    inner: Arc<State>,
}

impl<State> BlobRepo<State> {
    pub fn new(state: State) -> Self {
        Self {
            inner: Arc::new(state),
        }
    }
}

impl<State> BlobRepo<State>
where
    State: BlobState,
{
    pub fn get_blob(&self, key: &NodeHash) -> BoxFuture<Vec<u8>, Error> {
        fetch_blob_from_blobstore(self.inner.blobstore().clone(), *key)
    }
}

impl<State> Repo for BlobRepo<State>
where
    State: BlobState,
{
    fn get_changesets(&self) -> BoxStream<NodeHash, Error> {
        BlobChangesetStream {
            repo: BlobRepo {
                inner: self.inner.clone(),
            },
            heads: self.inner.heads().heads().boxify(),
            state: BCState::Idle,
            seen: HashSet::new(),
        }.boxify()
    }

    fn get_heads(&self) -> BoxStream<NodeHash, Error> {
        self.inner.heads().heads().boxify()
    }

    fn changeset_exists(&self, nodeid: &NodeHash) -> BoxFuture<bool, Error> {
        BlobChangeset::load(self.inner.blobstore(), nodeid)
            .map(|cs| cs.is_some())
            .boxify()
    }

    fn get_changeset_by_nodeid(&self, nodeid: &NodeHash) -> BoxFuture<Box<Changeset>, Error> {
        let nodeid = *nodeid;
        BlobChangeset::load(self.inner.blobstore(), &nodeid)
            .and_then(move |cs| {
                cs.ok_or(ErrorKind::ChangesetMissing(nodeid).into())
            })
            .map(|cs| cs.boxed())
            .boxify()
    }

    fn get_manifest_by_nodeid(&self, nodeid: &NodeHash) -> BoxFuture<Box<Manifest + Sync>, Error> {
        let nodeid = *nodeid;
        BlobManifest::load(self.inner.blobstore(), &nodeid)
            .and_then(move |mf| {
                mf.ok_or(ErrorKind::ManifestMissing(nodeid).into())
            })
            .map(|m| m.boxed())
            .boxify()
    }

    fn get_bookmark_keys(&self) -> BoxStream<Vec<u8>, Error> {
        self.inner.bookmarks().keys().boxify()
    }

    fn get_bookmark_value(
        &self,
        key: &AsRef<[u8]>,
    ) -> BoxFuture<Option<(NodeHash, Version)>, Error> {
        self.inner.bookmarks().get(key).boxify()
    }
}

impl<State> Clone for BlobRepo<State> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

pub struct BlobChangesetStream<State>
where
    State: BlobState,
{
    repo: BlobRepo<State>,
    seen: HashSet<NodeHash>,
    heads: BoxStream<NodeHash, Error>,
    state: BCState,
}

enum BCState {
    Idle,
    WaitCS(NodeHash, BoxFuture<Box<Changeset>, Error>),
}

impl<State> Stream for BlobChangesetStream<State>
where
    State: BlobState,
{
    type Item = NodeHash;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Error> {
        use self::BCState::*;

        loop {
            let (ret, state) = match &mut self.state {
                &mut Idle => {
                    if let Some(next) = try_ready!(self.heads.poll()) {
                        let state = if self.seen.insert(next) {
                            // haven't seen before
                            WaitCS(next, self.repo.get_changeset_by_nodeid(&next))
                        } else {
                            Idle // already done it
                        };

                        // Nothing to report, keep going
                        (None, state)
                    } else {
                        // Finished
                        (Some(None), Idle)
                    }
                }

                &mut WaitCS(ref next, ref mut csfut) => {
                    let cs = try_ready!(csfut.poll());

                    // get current heads stream and replace it with a placeholder
                    let heads = mem::replace(&mut self.heads, stream::empty().boxify());

                    // Add new heads - existing first, then new to get BFS
                    let parents = cs.parents().into_iter();
                    self.heads = heads.chain(stream::iter_ok(parents)).boxify();

                    (Some(Some(*next)), Idle)
                }
            };

            self.state = state;
            if let Some(ret) = ret {
                return Ok(Async::Ready(ret));
            }
        }
    }
}
