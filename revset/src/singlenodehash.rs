// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use futures::{Async, Poll};
use futures::future::Future;
use futures::stream::Stream;
use mercurial_types::{NodeHash, Repo};
use std::boxed::Box;

use NodeStream;
use errors::*;

pub struct SingleNodeHash {
    nodehash: Option<NodeHash>,
    exists: Box<Future<Item = bool, Error = Error> + Send>,
}

impl SingleNodeHash {
    pub fn new<R: Repo>(nodehash: NodeHash, repo: &R) -> Self {
        let exists = Box::new(repo.changeset_exists(&nodehash).map_err(move |e| {
            Error::with_chain(e, ErrorKind::RepoError(nodehash))
        }));
        let nodehash = Some(nodehash);
        SingleNodeHash { nodehash, exists }
    }

    pub fn boxed(self) -> Box<NodeStream> {
        Box::new(self)
    }
}

impl Stream for SingleNodeHash {
    type Item = NodeHash;
    type Error = Error;
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if self.nodehash.is_none() {
            Ok(Async::Ready(None))
        } else {
            match self.exists.poll()? {
                Async::NotReady => Ok(Async::NotReady),
                Async::Ready(true) => {
                    let nodehash = self.nodehash;
                    self.nodehash = None;
                    Ok(Async::Ready(nodehash))
                }
                Async::Ready(false) => Ok(Async::Ready(None)),
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use blobrepo::{BlobRepo, MemBlobState};
    use linear;
    use repoinfo::RepoGenCache;
    use std::sync::Arc;
    use tests::assert_node_sequence;
    use tests::string_to_nodehash;

    #[test]
    fn valid_node() {
        let repo = Arc::new(linear::getrepo());
        let nodestream = SingleNodeHash::new(
            string_to_nodehash("a5ffa77602a066db7d5cfb9fb5823a0895717c5a"),
            &repo,
        );

        let repo_generation: RepoGenCache<BlobRepo<MemBlobState>> = RepoGenCache::new(10);

        assert_node_sequence(
            repo_generation,
            &repo,
            vec![
                string_to_nodehash("a5ffa77602a066db7d5cfb9fb5823a0895717c5a"),
            ].into_iter(),
            nodestream.boxed(),
        );
    }

    #[test]
    fn invalid_node() {
        let repo = Arc::new(linear::getrepo());
        let nodehash = string_to_nodehash("0000000000000000000000000000000000000000");
        let nodestream = SingleNodeHash::new(nodehash, &repo).boxed();
        let repo_generation: RepoGenCache<BlobRepo<MemBlobState>> = RepoGenCache::new(10);

        assert_node_sequence(repo_generation, &repo, vec![].into_iter(), nodestream);
    }
}
