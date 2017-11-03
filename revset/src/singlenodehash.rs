// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use error_chain::ChainedError;
use futures::Poll;
use futures::future::Future;
use futures::stream::Stream;
use mercurial_types::{NodeHash, Repo};
use std::boxed::Box;

use NodeStream;
use errors::*;

pub struct SingleNodeHash {
    node: Box<Stream<Item = NodeHash, Error = Error>>,
}

impl SingleNodeHash {
    pub fn new<R>(nodehash: NodeHash, repo: &R) -> SingleNodeHash
    where
        R: Repo,
    {
        let future = repo.changeset_exists(&nodehash);
        let future = future.map_err(move |err| {
            ChainedError::with_chain(err, ErrorKind::NoSuchNode(nodehash))
        });
        let future = future.and_then(move |exists| if exists {
            Ok(nodehash)
        } else {
            Err(ErrorKind::NoSuchNode(nodehash).into())
        });
        SingleNodeHash {
            node: Box::new(future.into_stream()),
        }
    }

    pub fn boxed(self) -> Box<NodeStream> {
        Box::new(self)
    }
}

impl Stream for SingleNodeHash {
    type Item = NodeHash;
    type Error = Error;
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.node.poll()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use blobrepo::{BlobRepo, MemBlobState};
    use futures::executor::spawn;
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
        let repo = linear::getrepo();
        let nodehash = string_to_nodehash("0000000000000000000000000000000000000000");
        let mut nodestream = spawn(SingleNodeHash::new(nodehash, &repo));

        assert!(
            if let Some(Err(Error(ErrorKind::NoSuchNode(hash), _))) = nodestream.wait_stream() {
                hash == nodehash
            } else {
                false
            },
            "No error for bad node"
        );
    }
}
