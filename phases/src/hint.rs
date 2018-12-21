// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use Phase;
use blobrepo::BlobRepo;
use context::CoreContext;
use errors::*;
use futures::{future, stream, Future, Stream};
use futures_ext::{BoxFuture, FutureExt};
use mononoke_types::ChangesetId;
use reachabilityindex::ReachabilityIndex;
use reachabilityindex::SkiplistIndex;

#[derive(Clone)]
pub struct PhasesHint {
    index: SkiplistIndex,
}

impl PhasesHint {
    pub fn new() -> Self {
        Self {
            index: SkiplistIndex::new(),
        }
    }

    /// Retrieve the phase specified by this commit, if the commit exists
    /// Calculate it based on beeing ancestor of a public bookmark.
    /// Return error if calculation is unsuccessful due to any reason.
    pub fn get(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        cs_id: ChangesetId,
    ) -> BoxFuture<Phase, Error> {
        cloned!(self.index);
        repo.get_bonsai_bookmarks(ctx.clone())
            .collect()
            .and_then(move |vec| {
                let mut vecf = Vec::new();
                for (_, public_cs) in vec {
                    cloned!(ctx, index);
                    let changeset_fetcher = repo.get_changeset_fetcher();
                    vecf.push(index.query_reachability(ctx, changeset_fetcher, public_cs, cs_id));
                }
                stream::futures_unordered(vecf)
                    .skip_while(|&x| future::ok(!x))
                    .take(1)
                    .collect()
            })
            .map(|vec| {
                // vec should be size 0 or 1
                // if the changeset is ancestor of some public bookmark, it is public
                if vec.iter().any(|&x| x) {
                    Phase::Public
                } else {
                    // we can be sure that it is a draft commit
                    Phase::Draft
                }
            })
            .boxify()
    }
}
