// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::boxed::Box;

use blobrepo::BlobRepo;
use context::CoreContext;
use failure::Error;
use futures::future::Future;
use futures::stream::Stream;
use futures::{Async, Poll};
use mercurial_types::nodehash::HgChangesetId;
use mercurial_types::HgNodeHash;

use NodeStream;

pub struct SingleNodeHash {
    nodehash: Option<HgNodeHash>,
    exists: Box<Future<Item = bool, Error = Error> + Send>,
}

impl SingleNodeHash {
    pub fn new(ctx: CoreContext, nodehash: HgNodeHash, repo: &BlobRepo) -> Self {
        let changesetid = HgChangesetId::new(nodehash);
        let exists = Box::new(repo.changeset_exists(ctx, &changesetid));
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
    use context::CoreContext;
    use fixtures::linear;
    use futures_ext::StreamExt;
    use std::sync::Arc;
    use tests::assert_node_sequence;
    use tests::string_to_nodehash;

    #[test]
    fn valid_node() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = Arc::new(linear::getrepo(None));
            let nodestream = SingleNodeHash::new(
                ctx.clone(),
                string_to_nodehash("a5ffa77602a066db7d5cfb9fb5823a0895717c5a"),
                &repo,
            );

            assert_node_sequence(
                ctx,
                &repo,
                vec![string_to_nodehash(
                    "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
                )]
                .into_iter(),
                nodestream.boxify(),
            );
        });
    }

    #[test]
    fn invalid_node() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = Arc::new(linear::getrepo(None));
            let nodehash = string_to_nodehash("1000000000000000000000000000000000000000");
            let nodestream = SingleNodeHash::new(ctx.clone(), nodehash, &repo).boxify();

            assert_node_sequence(ctx, &repo, vec![].into_iter(), nodestream);
        });
    }
}
