// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::error;
use std::marker::PhantomData;
use std::sync::Arc;

use futures::future::Future;
use futures::stream::Stream;

use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};

use changeset::Changeset;
use manifest::{BoxManifest, Manifest};
use nodehash::NodeHash;
use storage_types::Version;

pub trait Repo: Send + Sync + 'static {
    type Error: error::Error + Send + 'static;

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
    fn get_bookmark_keys(&self) -> BoxStream<Vec<u8>, Self::Error>;
    fn get_bookmark_value(
        &self,
        key: &AsRef<[u8]>,
    ) -> BoxFuture<Option<(NodeHash, Version)>, Self::Error>;
    fn changeset_exists(&self, nodeid: &NodeHash) -> BoxFuture<bool, Self::Error>;
    fn get_changeset_by_nodeid(&self, nodeid: &NodeHash) -> BoxFuture<Box<Changeset>, Self::Error>;
    fn get_manifest_by_nodeid(
        &self,
        nodeid: &NodeHash,
    ) -> BoxFuture<Box<Manifest<Error = Self::Error> + Sync>, Self::Error>;

    fn boxed(self) -> Box<Repo<Error = Self::Error> + Sync>
    where
        Self: Sync + Sized,
    {
        Box::new(self)
    }
}

pub struct BoxRepo<R, E>
where
    R: Repo,
{
    repo: R,
    cvterr: fn(R::Error) -> E,
    _phantom: PhantomData<E>,
}

// The box can be Sync iff R is Sync, E doesn't matter as its phantom
unsafe impl<R, E> Sync for BoxRepo<R, E>
where
    R: Repo + Sync,
{
}

impl<R, E> BoxRepo<R, E>
where
    R: Repo + Sync + Send,
    E: error::Error + Send + 'static,
{
    pub fn new(repo: R) -> Box<Repo<Error = E> + Sync + Send>
    where
        E: From<R::Error>,
    {
        Self::new_with_cvterr(repo, E::from)
    }

    pub fn new_with_cvterr(
        repo: R,
        cvterr: fn(R::Error) -> E,
    ) -> Box<Repo<Error = E> + Sync + Send> {
        let br = BoxRepo {
            repo,
            cvterr,
            _phantom: PhantomData,
        };

        Box::new(br)
    }
}

impl<R, E> Repo for BoxRepo<R, E>
where
    R: Repo + Sync + Send + 'static,
    E: error::Error + Send + 'static,
{
    type Error = E;

    fn get_changesets(&self) -> BoxStream<NodeHash, Self::Error> {
        self.repo.get_changesets().map_err(self.cvterr).boxify()
    }

    fn get_heads(&self) -> BoxStream<NodeHash, Self::Error> {
        self.repo.get_heads().map_err(self.cvterr).boxify()
    }

    fn get_bookmark_keys(&self) -> BoxStream<Vec<u8>, Self::Error> {
        self.repo.get_bookmark_keys().map_err(self.cvterr).boxify()
    }

    fn get_bookmark_value(
        &self,
        key: &AsRef<[u8]>,
    ) -> BoxFuture<Option<(NodeHash, Version)>, Self::Error> {
        self.repo
            .get_bookmark_value(key)
            .map_err(self.cvterr)
            .boxify()
    }

    fn changeset_exists(&self, nodeid: &NodeHash) -> BoxFuture<bool, Self::Error> {
        let cvterr = self.cvterr;

        self.repo.changeset_exists(nodeid).map_err(cvterr).boxify()
    }

    fn get_changeset_by_nodeid(&self, nodeid: &NodeHash) -> BoxFuture<Box<Changeset>, Self::Error> {
        let cvterr = self.cvterr;

        self.repo
            .get_changeset_by_nodeid(nodeid)
            .map_err(cvterr)
            .boxify()
    }

    fn get_manifest_by_nodeid(
        &self,
        nodeid: &NodeHash,
    ) -> BoxFuture<Box<Manifest<Error = Self::Error> + Sync>, Self::Error> {
        let cvterr = self.cvterr;

        self.repo
            .get_manifest_by_nodeid(nodeid)
            .map(move |m| BoxManifest::new_with_cvterr(m, cvterr))
            .map_err(cvterr)
            .boxify()
    }
}


impl<RE> Repo for Box<Repo<Error = RE> + Sync + Send>
where
    RE: error::Error + Send + 'static,
{
    type Error = RE;

    fn get_changesets(&self) -> BoxStream<NodeHash, Self::Error> {
        (**self).get_changesets()
    }

    fn get_heads(&self) -> BoxStream<NodeHash, Self::Error> {
        (**self).get_heads()
    }

    fn get_bookmark_keys(&self) -> BoxStream<Vec<u8>, Self::Error> {
        (**self).get_bookmark_keys()
    }

    fn get_bookmark_value(
        &self,
        key: &AsRef<[u8]>,
    ) -> BoxFuture<Option<(NodeHash, Version)>, Self::Error> {
        (**self).get_bookmark_value(key)
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
    ) -> BoxFuture<Box<Manifest<Error = Self::Error> + Sync>, Self::Error> {
        (**self).get_manifest_by_nodeid(nodeid)
    }
}

impl<R> Repo for Box<R>
where
    R: Repo,
{
    type Error = R::Error;

    fn get_changesets(&self) -> BoxStream<NodeHash, Self::Error> {
        (**self).get_changesets()
    }

    fn get_heads(&self) -> BoxStream<NodeHash, Self::Error> {
        (**self).get_heads()
    }

    fn get_bookmark_keys(&self) -> BoxStream<Vec<u8>, Self::Error> {
        (**self).get_bookmark_keys()
    }

    fn get_bookmark_value(
        &self,
        key: &AsRef<[u8]>,
    ) -> BoxFuture<Option<(NodeHash, Version)>, Self::Error> {
        (**self).get_bookmark_value(key)
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
    ) -> BoxFuture<Box<Manifest<Error = Self::Error> + Sync>, Self::Error> {
        (**self).get_manifest_by_nodeid(nodeid)
    }
}

impl<RE> Repo for Arc<Repo<Error = RE>>
where
    RE: error::Error + Send + 'static,
{
    type Error = RE;

    fn get_changesets(&self) -> BoxStream<NodeHash, Self::Error> {
        (**self).get_changesets()
    }

    fn get_heads(&self) -> BoxStream<NodeHash, Self::Error> {
        (**self).get_heads()
    }

    fn get_bookmark_keys(&self) -> BoxStream<Vec<u8>, Self::Error> {
        (**self).get_bookmark_keys()
    }

    fn get_bookmark_value(
        &self,
        key: &AsRef<[u8]>,
    ) -> BoxFuture<Option<(NodeHash, Version)>, Self::Error> {
        (**self).get_bookmark_value(key)
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
    ) -> BoxFuture<Box<Manifest<Error = Self::Error> + Sync>, Self::Error> {
        (**self).get_manifest_by_nodeid(nodeid)
    }
}

impl<R> Repo for Arc<R>
where
    R: Repo,
{
    type Error = R::Error;

    fn get_changesets(&self) -> BoxStream<NodeHash, Self::Error> {
        (**self).get_changesets()
    }

    fn get_heads(&self) -> BoxStream<NodeHash, Self::Error> {
        (**self).get_heads()
    }

    fn get_bookmark_keys(&self) -> BoxStream<Vec<u8>, Self::Error> {
        (**self).get_bookmark_keys()
    }

    fn get_bookmark_value(
        &self,
        key: &AsRef<[u8]>,
    ) -> BoxFuture<Option<(NodeHash, Version)>, Self::Error> {
        (**self).get_bookmark_value(key)
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
    ) -> BoxFuture<Box<Manifest<Error = Self::Error> + Sync>, Self::Error> {
        (**self).get_manifest_by_nodeid(nodeid)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[derive(Copy, Clone)]
    struct DummyRepo;

    impl Repo for DummyRepo {
        type Error = !;

        fn get_changesets(&self) -> BoxStream<NodeHash, Self::Error> {
            unimplemented!("dummy impl")
        }

        fn get_heads(&self) -> BoxStream<NodeHash, Self::Error> {
            unimplemented!("dummy impl")
        }

        fn get_bookmark_keys(&self) -> BoxStream<Vec<u8>, Self::Error> {
            unimplemented!("dummy impl")
        }

        fn get_bookmark_value(
            &self,
            _key: &AsRef<[u8]>,
        ) -> BoxFuture<Option<(NodeHash, Version)>, Self::Error> {
            unimplemented!("dummy impl")
        }

        fn changeset_exists(&self, _nodeid: &NodeHash) -> BoxFuture<bool, Self::Error> {
            unimplemented!("dummy impl")
        }

        fn get_changeset_by_nodeid(
            &self,
            _nodeid: &NodeHash,
        ) -> BoxFuture<Box<Changeset>, Self::Error> {
            unimplemented!("dummy impl")
        }

        fn get_manifest_by_nodeid(
            &self,
            _nodeid: &NodeHash,
        ) -> BoxFuture<Box<Manifest<Error = Self::Error> + Sync>, Self::Error> {
            unimplemented!("dummy impl")
        }
    }

    #[test]
    fn test_impl() {
        fn _assert_repo<T: Repo>(_: &T) {}

        let repo = DummyRepo;
        let a = Arc::new(repo);
        let b = Box::new(repo);

        _assert_repo(&repo);
        _assert_repo(&a);
        _assert_repo(&(a as Arc<Repo<Error = !>>));
        _assert_repo(&b);
        _assert_repo(&(b as Box<Repo<Error = !> + Sync + Send>));
    }
}
