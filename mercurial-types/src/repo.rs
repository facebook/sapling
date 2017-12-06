// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::sync::Arc;

use futures::Future;

use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};

use changeset::Changeset;
use manifest::{BoxManifest, Manifest};
use nodehash::NodeHash;
use storage_types::Version;

use errors::*;

pub trait Repo: Send + Sync + 'static {
    /// Return a stream of all changeset ids
    ///
    /// This returns a Stream which produces each changeset that's reachable from a
    /// head exactly once. This does not guarantee any particular order, but something
    /// approximating a BFS traversal from the heads would be ideal.
    ///
    /// XXX Is "exactly once" too strong? This probably requires a "has seen" structure which
    /// will be O(changesets) in size. Probably OK up to 10-100M changesets.
    fn get_changesets(&self) -> BoxStream<NodeHash, Error>;

    fn get_heads(&self) -> BoxStream<NodeHash, Error>;
    fn get_bookmark_keys(&self) -> BoxStream<Vec<u8>, Error>;
    fn get_bookmark_value(
        &self,
        key: &AsRef<[u8]>,
    ) -> BoxFuture<Option<(NodeHash, Version)>, Error>;
    fn changeset_exists(&self, nodeid: &NodeHash) -> BoxFuture<bool, Error>;
    fn get_changeset_by_nodeid(&self, nodeid: &NodeHash) -> BoxFuture<Box<Changeset>, Error>;
    fn get_manifest_by_nodeid(&self, nodeid: &NodeHash) -> BoxFuture<Box<Manifest + Sync>, Error>;

    fn boxed(self) -> Box<Repo + Sync>
    where
        Self: Sync + Sized,
    {
        Box::new(self)
    }
}

pub struct BoxRepo<R>
where
    R: Repo,
{
    repo: R,
}

impl<R> BoxRepo<R>
where
    R: Repo + Sync + Send,
{
    pub fn new(repo: R) -> Box<Repo + Sync + Send> {
        let br = BoxRepo { repo };

        Box::new(br)
    }
}

impl<R> Repo for BoxRepo<R>
where
    R: Repo + Sync + Send + 'static,
{
    fn get_changesets(&self) -> BoxStream<NodeHash, Error> {
        self.repo.get_changesets().boxify()
    }

    fn get_heads(&self) -> BoxStream<NodeHash, Error> {
        self.repo.get_heads().boxify()
    }

    fn get_bookmark_keys(&self) -> BoxStream<Vec<u8>, Error> {
        self.repo.get_bookmark_keys().boxify()
    }

    fn get_bookmark_value(
        &self,
        key: &AsRef<[u8]>,
    ) -> BoxFuture<Option<(NodeHash, Version)>, Error> {
        self.repo.get_bookmark_value(key).boxify()
    }

    fn changeset_exists(&self, nodeid: &NodeHash) -> BoxFuture<bool, Error> {
        self.repo.changeset_exists(nodeid).boxify()
    }

    fn get_changeset_by_nodeid(&self, nodeid: &NodeHash) -> BoxFuture<Box<Changeset>, Error> {
        self.repo.get_changeset_by_nodeid(nodeid).boxify()
    }

    fn get_manifest_by_nodeid(&self, nodeid: &NodeHash) -> BoxFuture<Box<Manifest + Sync>, Error> {
        self.repo
            .get_manifest_by_nodeid(nodeid)
            .map(move |m| BoxManifest::new(m))
            .boxify()
    }
}


impl Repo for Box<Repo + Sync + Send> {
    fn get_changesets(&self) -> BoxStream<NodeHash, Error> {
        (**self).get_changesets()
    }

    fn get_heads(&self) -> BoxStream<NodeHash, Error> {
        (**self).get_heads()
    }

    fn get_bookmark_keys(&self) -> BoxStream<Vec<u8>, Error> {
        (**self).get_bookmark_keys()
    }

    fn get_bookmark_value(
        &self,
        key: &AsRef<[u8]>,
    ) -> BoxFuture<Option<(NodeHash, Version)>, Error> {
        (**self).get_bookmark_value(key)
    }

    fn changeset_exists(&self, nodeid: &NodeHash) -> BoxFuture<bool, Error> {
        (**self).changeset_exists(nodeid)
    }

    fn get_changeset_by_nodeid(&self, nodeid: &NodeHash) -> BoxFuture<Box<Changeset>, Error> {
        (**self).get_changeset_by_nodeid(nodeid)
    }

    fn get_manifest_by_nodeid(&self, nodeid: &NodeHash) -> BoxFuture<Box<Manifest + Sync>, Error> {
        (**self).get_manifest_by_nodeid(nodeid)
    }
}

impl<R> Repo for Box<R>
where
    R: Repo,
{
    fn get_changesets(&self) -> BoxStream<NodeHash, Error> {
        (**self).get_changesets()
    }

    fn get_heads(&self) -> BoxStream<NodeHash, Error> {
        (**self).get_heads()
    }

    fn get_bookmark_keys(&self) -> BoxStream<Vec<u8>, Error> {
        (**self).get_bookmark_keys()
    }

    fn get_bookmark_value(
        &self,
        key: &AsRef<[u8]>,
    ) -> BoxFuture<Option<(NodeHash, Version)>, Error> {
        (**self).get_bookmark_value(key)
    }

    fn changeset_exists(&self, nodeid: &NodeHash) -> BoxFuture<bool, Error> {
        (**self).changeset_exists(nodeid)
    }

    fn get_changeset_by_nodeid(&self, nodeid: &NodeHash) -> BoxFuture<Box<Changeset>, Error> {
        (**self).get_changeset_by_nodeid(nodeid)
    }

    fn get_manifest_by_nodeid(&self, nodeid: &NodeHash) -> BoxFuture<Box<Manifest + Sync>, Error> {
        (**self).get_manifest_by_nodeid(nodeid)
    }
}

impl Repo for Arc<Repo> {
    fn get_changesets(&self) -> BoxStream<NodeHash, Error> {
        (**self).get_changesets()
    }

    fn get_heads(&self) -> BoxStream<NodeHash, Error> {
        (**self).get_heads()
    }

    fn get_bookmark_keys(&self) -> BoxStream<Vec<u8>, Error> {
        (**self).get_bookmark_keys()
    }

    fn get_bookmark_value(
        &self,
        key: &AsRef<[u8]>,
    ) -> BoxFuture<Option<(NodeHash, Version)>, Error> {
        (**self).get_bookmark_value(key)
    }

    fn changeset_exists(&self, nodeid: &NodeHash) -> BoxFuture<bool, Error> {
        (**self).changeset_exists(nodeid)
    }

    fn get_changeset_by_nodeid(&self, nodeid: &NodeHash) -> BoxFuture<Box<Changeset>, Error> {
        (**self).get_changeset_by_nodeid(nodeid)
    }

    fn get_manifest_by_nodeid(&self, nodeid: &NodeHash) -> BoxFuture<Box<Manifest + Sync>, Error> {
        (**self).get_manifest_by_nodeid(nodeid)
    }
}

impl<R> Repo for Arc<R>
where
    R: Repo,
{
    fn get_changesets(&self) -> BoxStream<NodeHash, Error> {
        (**self).get_changesets()
    }

    fn get_heads(&self) -> BoxStream<NodeHash, Error> {
        (**self).get_heads()
    }

    fn get_bookmark_keys(&self) -> BoxStream<Vec<u8>, Error> {
        (**self).get_bookmark_keys()
    }

    fn get_bookmark_value(
        &self,
        key: &AsRef<[u8]>,
    ) -> BoxFuture<Option<(NodeHash, Version)>, Error> {
        (**self).get_bookmark_value(key)
    }

    fn changeset_exists(&self, nodeid: &NodeHash) -> BoxFuture<bool, Error> {
        (**self).changeset_exists(nodeid)
    }

    fn get_changeset_by_nodeid(&self, nodeid: &NodeHash) -> BoxFuture<Box<Changeset>, Error> {
        (**self).get_changeset_by_nodeid(nodeid)
    }

    fn get_manifest_by_nodeid(&self, nodeid: &NodeHash) -> BoxFuture<Box<Manifest + Sync>, Error> {
        (**self).get_manifest_by_nodeid(nodeid)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[derive(Copy, Clone)]
    struct DummyRepo;

    impl Repo for DummyRepo {
        fn get_changesets(&self) -> BoxStream<NodeHash, Error> {
            unimplemented!("dummy impl")
        }

        fn get_heads(&self) -> BoxStream<NodeHash, Error> {
            unimplemented!("dummy impl")
        }

        fn get_bookmark_keys(&self) -> BoxStream<Vec<u8>, Error> {
            unimplemented!("dummy impl")
        }

        fn get_bookmark_value(
            &self,
            _key: &AsRef<[u8]>,
        ) -> BoxFuture<Option<(NodeHash, Version)>, Error> {
            unimplemented!("dummy impl")
        }

        fn changeset_exists(&self, _nodeid: &NodeHash) -> BoxFuture<bool, Error> {
            unimplemented!("dummy impl")
        }

        fn get_changeset_by_nodeid(&self, _nodeid: &NodeHash) -> BoxFuture<Box<Changeset>, Error> {
            unimplemented!("dummy impl")
        }

        fn get_manifest_by_nodeid(
            &self,
            _nodeid: &NodeHash,
        ) -> BoxFuture<Box<Manifest + Sync>, Error> {
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
        _assert_repo(&(a as Arc<Repo>));
        _assert_repo(&b);
        _assert_repo(&(b as Box<Repo + Sync + Send>));
    }
}
