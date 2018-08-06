// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::boxed::Box;

use blobrepo::BlobRepo;
use failure::Error;
use futures::{Async, Poll};
use futures::future::Future;
use futures::stream::Stream;
use mercurial_types::HgNodeHash;
use mercurial_types::nodehash::HgChangesetId;

use NodeStream;

pub struct SingleNodeHash {
    nodehash: Option<HgNodeHash>,
    exists: Box<Future<Item = bool, Error = Error> + Send>,
}

impl SingleNodeHash {
    pub fn new(nodehash: HgNodeHash, repo: &BlobRepo) -> Self {
        let changesetid = HgChangesetId::new(nodehash);
        let exists = Box::new(repo.changeset_exists(&changesetid));
        let nodehash = Some(nodehash);
        SingleNodeHash { nodehash, exists }
    }

    pub fn boxed(self) -> Box<NodeStream> {
        Box::new(self)
    }
}

impl Stream for SingleNodeHash {
    type Item = HgNodeHash;
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
    use async_unit;
    use fixtures::linear;
    use std::sync::Arc;
    use tests::assert_node_sequence;
    use tests::string_to_nodehash;

    #[test]
    fn valid_node() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(linear::getrepo(None));
            let nodestream = SingleNodeHash::new(
                string_to_nodehash("a5ffa77602a066db7d5cfb9fb5823a0895717c5a"),
                &repo,
            );

            assert_node_sequence(
                &repo,
                vec![
                    string_to_nodehash("a5ffa77602a066db7d5cfb9fb5823a0895717c5a"),
                ].into_iter(),
                nodestream.boxed(),
            );
        });
    }

    #[test]
    fn invalid_node() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(linear::getrepo(None));
            let nodehash = string_to_nodehash("1000000000000000000000000000000000000000");
            let nodestream = SingleNodeHash::new(nodehash, &repo).boxed();

            assert_node_sequence(&repo, vec![].into_iter(), nodestream);
        });
    }
}
