// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::boxed::Box;

use blobrepo::BlobRepo;
use context::CoreContext;
use failure::Error;
use futures::{Async, Poll};
use futures::future::Future;
use futures::stream::Stream;
use mercurial_types::HgNodeHash;
use mercurial_types::nodehash::HgChangesetId;
use mononoke_types::ChangesetId;

use BonsaiNodeStream;

pub struct SingleChangesetId {
    hg_cs_id: Option<HgChangesetId>,
    repo: BlobRepo,
    ctx: CoreContext,
}

impl SingleChangesetId {
    pub fn new(ctx: CoreContext, nodehash: HgNodeHash, repo: &BlobRepo) -> Self {
        Self {
            hg_cs_id: Some(HgChangesetId::new(nodehash)),
            repo: repo.clone(),
            ctx: ctx,
        }
    }

    pub fn boxed(self) -> Box<BonsaiNodeStream> {
        Box::new(self)
    }
}

impl Stream for SingleChangesetId {
    type Item = ChangesetId;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let ctx = self.ctx.clone();
        match self.hg_cs_id {
            None => Ok(Async::Ready(None)),
            Some(hg_cs_id) => match self.repo.get_bonsai_from_hg(ctx, &hg_cs_id).poll()? {
                Async::NotReady => Ok(Async::NotReady),
                Async::Ready(option) => {
                    self.hg_cs_id = None;
                    Ok(Async::Ready(option))
                }
            },
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use async_unit;
    use context::CoreContext;
    use fixtures::linear;
    use std::sync::Arc;
    use tests::assert_changesets_sequence;
    use tests::string_to_bonsai;
    use tests::string_to_nodehash;

    #[test]
    fn valid_changeset() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = Arc::new(linear::getrepo(None));
            let changeset_stream = SingleChangesetId::new(
                ctx.clone(),
                string_to_nodehash("a5ffa77602a066db7d5cfb9fb5823a0895717c5a"),
                &repo,
            ).boxed();

            assert_changesets_sequence(
                ctx.clone(),
                &repo,
                vec![
                    string_to_bonsai(
                        ctx.clone(),
                        &repo,
                        "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
                    ),
                ].into_iter(),
                changeset_stream,
            );
        });
    }

    #[test]
    fn invalid_changeset() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = Arc::new(linear::getrepo(None));
            let nodehash = string_to_nodehash("1000000000000000000000000000000000000000");
            let changeset_stream = SingleChangesetId::new(ctx.clone(), nodehash, &repo).boxed();

            assert_changesets_sequence(ctx, &repo, vec![].into_iter(), changeset_stream);
        });
    }
}
