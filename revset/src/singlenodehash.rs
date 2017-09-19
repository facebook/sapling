// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use futures::Poll;
use futures::future::Future;
use futures::stream::Stream;
use mercurial_types::{NodeHash, Repo};
use std::boxed::Box;

use RevsetError;

pub struct SingleNodeHash {
    node: Box<Stream<Item = NodeHash, Error = RevsetError>>,
}

impl SingleNodeHash {
    pub fn new<R>(nodehash: NodeHash, repo: &R) -> SingleNodeHash
    where
        R: Repo,
    {
        let future = repo.changeset_exists(&nodehash);
        let future = future.map_err(move |_| RevsetError::NoSuchNode(nodehash));
        let future = future.and_then(move |exists| if exists {
            Ok(nodehash)
        } else {
            Err(RevsetError::NoSuchNode(nodehash))
        });
        SingleNodeHash {
            node: Box::new(future.into_stream()),
        }
    }
}

impl Stream for SingleNodeHash {
    type Item = NodeHash;
    type Error = RevsetError;
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.node.poll()
    }
}

#[cfg(test)]
mod test {
    use super::{RevsetError, SingleNodeHash};
    use assert_node_sequence;
    use futures::executor::spawn;
    use linear;
    use string_to_nodehash;

    #[test]
    fn valid_node() {
        let repo = linear::getrepo();
        let nodestream = SingleNodeHash::new(
            string_to_nodehash("a5ffa77602a066db7d5cfb9fb5823a0895717c5a"),
            &repo,
        );

        assert_node_sequence(
            vec![
                string_to_nodehash("a5ffa77602a066db7d5cfb9fb5823a0895717c5a"),
            ].into_iter(),
            Box::new(nodestream),
        );
    }

    #[test]
    fn invalid_node() {
        let repo = linear::getrepo();
        let nodehash = string_to_nodehash("0000000000000000000000000000000000000000");
        let mut nodestream = spawn(SingleNodeHash::new(nodehash, &repo));

        assert!(
            if let Some(Err(RevsetError::NoSuchNode(hash))) = nodestream.wait_stream() {
                hash == nodehash
            } else {
                false
            },
            "No error for bad node"
        );
    }
}
