// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::sync::Arc;

use futures::future::BoxFuture;
use futures::stream::BoxStream;

use manifest::Manifest;
use bookmarks::{Bookmarks, Version};
use changeset::Changeset;
use nodehash::NodeHash;

pub type BoxedBookmarks<E> = Box<
    Bookmarks<
        Error=E,
        Value=NodeHash,
        Get=BoxFuture<Option<(NodeHash, Version)>, E>,
        Keys=BoxStream<Vec<u8>, E>,
    >
>;

pub trait Repo: 'static {
    type Error: Send + 'static;

    /// Return a stream of all changeset ids
    ///
    /// This returns a Stream which produces each changeset that's reachable from a
    /// head exactly once. This does not guarantee any particular order, but something
    /// approximating a BFS traversal from the heads would be ideal.
    ///
    /// XXX Is "exactly once" too strong? This probably requires a "has seen" structure which
    /// will be O(changesets) in size. Probably OK up to 10-100M changesets.
    fn get_changesets(&self) -> BoxStream<NodeHash, Self::Error>;

    fn get_heads(&self) -> BoxStream<NodeHash, Self::Error>;
    fn get_bookmarks(&self) -> Result<BoxedBookmarks<Self::Error>, Self::Error>;
    fn changeset_exists(&self, nodeid: &NodeHash) -> BoxFuture<bool, Self::Error>;
    fn get_changeset_by_nodeid(&self, nodeid: &NodeHash) -> BoxFuture<Box<Changeset>, Self::Error>;
    fn get_manifest_by_nodeid(
        &self,
        nodeid: &NodeHash,
    ) -> BoxFuture<Box<Manifest<Error = Self::Error>>, Self::Error>;

    fn boxed(self) -> Box<Repo<Error = Self::Error>>
    where
        Self: Sized,
    {
        Box::new(self)
    }
}

impl<RE> Repo for Box<Repo<Error = RE>>
where
    RE: Send + 'static,
{
    type Error = RE;

    fn get_changesets(&self) -> BoxStream<NodeHash, Self::Error> {
        (**self).get_changesets()
    }

    fn get_heads(&self) -> BoxStream<NodeHash, Self::Error> {
        (**self).get_heads()
    }

    fn get_bookmarks(&self) -> Result<BoxedBookmarks<Self::Error>, Self::Error> {
        (**self).get_bookmarks()
    }

    fn changeset_exists(&self, nodeid: &NodeHash) -> BoxFuture<bool, Self::Error> {
        (**self).changeset_exists(nodeid)
    }

    fn get_changeset_by_nodeid(&self, nodeid: &NodeHash) -> BoxFuture<Box<Changeset>, Self::Error> {
        (**self).get_changeset_by_nodeid(nodeid)
    }

    fn get_manifest_by_nodeid(
        &self,
        nodeid: &NodeHash,
    ) -> BoxFuture<Box<Manifest<Error = Self::Error>>, Self::Error> {
        (**self).get_manifest_by_nodeid(nodeid)
    }
}

impl<RE> Repo for Arc<Repo<Error = RE>>
where
    RE: Send + 'static,
{
    type Error = RE;

    fn get_changesets(&self) -> BoxStream<NodeHash, Self::Error> {
        (**self).get_changesets()
    }

    fn get_heads(&self) -> BoxStream<NodeHash, Self::Error> {
        (**self).get_heads()
    }

    fn get_bookmarks(&self) -> Result<BoxedBookmarks<Self::Error>, Self::Error> {
        (**self).get_bookmarks()
    }

    fn changeset_exists(&self, nodeid: &NodeHash) -> BoxFuture<bool, Self::Error> {
        (**self).changeset_exists(nodeid)
    }

    fn get_changeset_by_nodeid(&self, nodeid: &NodeHash) -> BoxFuture<Box<Changeset>, Self::Error> {
        (**self).get_changeset_by_nodeid(nodeid)
    }

    fn get_manifest_by_nodeid(
        &self,
        nodeid: &NodeHash,
    ) -> BoxFuture<Box<Manifest<Error = Self::Error>>, Self::Error> {
        (**self).get_manifest_by_nodeid(nodeid)
    }
}
