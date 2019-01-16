// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobrepo::ChangesetFetcher;
use context::CoreContext;
use errors::*;
use futures::{future, stream, Future, Stream};
use futures_ext::{BoxFuture, FutureExt};
use mononoke_types::ChangesetId;
use reachabilityindex::ReachabilityIndex;
use reachabilityindex::SkiplistIndex;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use Phase;

#[derive(Clone)]
pub struct PhasesReachabilityHint {
    index: Arc<SkiplistIndex>,
}

impl PhasesReachabilityHint {
    pub fn new(skip_index: Arc<SkiplistIndex>) -> Self {
        Self { index: skip_index }
    }

    /// Retrieve the phase specified by this commit, if the commit exists
    /// Calculate it based on beeing ancestor of a public bookmark.
    /// Return error if calculation is unsuccessful due to any reason.
    pub fn get(
        &self,
        ctx: CoreContext,
        changeset_fetcher: Arc<ChangesetFetcher>,
        cs_id: ChangesetId,
        bookmarks_cs_ids: Arc<HashSet<ChangesetId>>,
    ) -> BoxFuture<Phase, Error> {
        if bookmarks_cs_ids.contains(&cs_id) {
            return future::ok(Phase::Public).boxify();
        }
        let mut vecf = Vec::new();
        for public_cs in bookmarks_cs_ids.iter() {
            cloned!(ctx, self.index, changeset_fetcher);
            vecf.push(index.query_reachability(ctx, changeset_fetcher, public_cs.clone(), cs_id));
        }
        stream::futures_unordered(vecf)
            .skip_while(|&x| future::ok(!x))
            .take(1)
            .collect()
            .map(|vec| {
                if vec.is_empty() {
                    Phase::Draft
                } else {
                    Phase::Public
                }
            })
            .boxify()
    }

    /// Retrieve the phases for set of commits.
    /// Calculate it based on beeing ancestor of a public bookmark.
    /// Return error if calculation is unsuccessful due to any reason.
    /// The resulting hashmap contains phases for all the input commits.
    /// Number of public bookmarks for a repo can be huge.
    pub fn get_all(
        &self,
        ctx: CoreContext,
        changeset_fetcher: Arc<ChangesetFetcher>,
        cs_ids: Vec<ChangesetId>,
        bookmarks_cs_ids: Arc<HashSet<ChangesetId>>,
    ) -> BoxFuture<HashMap<ChangesetId, Phase>, Error> {
        stream::futures_unordered(cs_ids.into_iter().map(|cs_id| {
            cloned!(ctx, changeset_fetcher, bookmarks_cs_ids);
            self.get(ctx, changeset_fetcher, cs_id, bookmarks_cs_ids)
                .map(move |phase| (cs_id, phase))
        }))
        .collect()
        .map(move |vec| vec.into_iter().collect::<HashMap<_, _>>())
        .boxify()
    }
}
