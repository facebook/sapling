// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use changeset_fetcher::ChangesetFetcher;
use context::CoreContext;
use errors::*;
use futures::{future, stream, Future, Stream};
use futures_ext::{BoxFuture, FutureExt};
use mononoke_types::ChangesetId;
use reachabilityindex::{LeastCommonAncestorsHint, NodeFrontier};
use skiplist::SkiplistIndex;
use std::collections::{HashMap, HashSet};
use std::iter::FromIterator;
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
            cloned!(ctx, changeset_fetcher);
            vecf.push(
                changeset_fetcher
                    .get_generation_number(ctx, public_cs.clone())
                    .map({
                        cloned!(public_cs);
                        move |gen_num| (public_cs, gen_num)
                    }),
            );
        }

        let cs_and_gen_num = changeset_fetcher
            .get_generation_number(ctx.clone(), cs_id.clone())
            .map({
                cloned!(cs_id);
                move |gen_num| (cs_id, gen_num)
            });

        cloned!(self.index);
        stream::futures_unordered(vecf)
            .collect()
            .join(cs_and_gen_num)
            .and_then(move |(heads_and_gen_nums, (cs_id, gen_num))| {
                let nf = NodeFrontier::from_iter(heads_and_gen_nums);
                index
                    .lca_hint(ctx.clone(), changeset_fetcher, nf, gen_num)
                    .map(move |moved_node_frontier| {
                        if let Some(cs_ids_for_gen_num) = moved_node_frontier.get(&gen_num) {
                            if cs_ids_for_gen_num.contains(&cs_id) {
                                return Phase::Public;
                            }
                        }

                        Phase::Draft
                    })
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
