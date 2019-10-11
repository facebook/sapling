/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use cloned::cloned;
use context::CoreContext;
use futures::{future::ok, Future};
use futures_ext::{BoxFuture, FutureExt};
use mercurial_types::HgChangesetId;
use mononoke_types::{ChangesetId, RepositoryId};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::{
    errors::Error, filter_fetched_ids, BonsaiHgMapping, BonsaiHgMappingEntry,
    BonsaiOrHgChangesetIds,
};

/// A bonsai <-> hg mapping wrapper that reads from the underlying mapping but writes to memory.
/// It can be used to prevent a piece of code to do writes to the db but at the same time give
/// this piece of code a consistent view of the repository.
#[derive(Clone)]
pub struct MemWritesBonsaiHgMapping {
    mappings: Arc<Mutex<InMemoryMappings>>,

    inner: Arc<dyn BonsaiHgMapping>,
}

struct InMemoryMappings {
    hg_to_bcs: HashMap<(RepositoryId, HgChangesetId), ChangesetId>,
    bcs_to_hg: HashMap<(RepositoryId, ChangesetId), HgChangesetId>,
    ordered_inserts: Vec<BonsaiHgMappingEntry>,
}

impl InMemoryMappings {
    fn new() -> Self {
        Self {
            hg_to_bcs: HashMap::new(),
            bcs_to_hg: HashMap::new(),
            ordered_inserts: vec![],
        }
    }
}

impl MemWritesBonsaiHgMapping {
    pub fn new(inner: Arc<dyn BonsaiHgMapping>) -> Self {
        Self {
            mappings: Arc::new(Mutex::new(InMemoryMappings::new())),
            inner,
        }
    }

    /// Get all mapping items that were inserted in the same order as they were inserted
    pub fn get_ordered_inserts(&self) -> Vec<BonsaiHgMappingEntry> {
        let mappings = self.mappings.lock().unwrap();
        mappings.ordered_inserts.clone()
    }

    pub fn get_inner(&self) -> Arc<dyn BonsaiHgMapping> {
        self.inner.clone()
    }
}

impl BonsaiHgMapping for MemWritesBonsaiHgMapping {
    fn add(&self, ctx: CoreContext, entry: BonsaiHgMappingEntry) -> BoxFuture<bool, Error> {
        let repo_id = entry.repo_id;
        {
            let mappings = self.mappings.lock().expect("lock poisoned");

            let bcs_to_hg = &mappings.bcs_to_hg;

            if bcs_to_hg.contains_key(&(repo_id, entry.bcs_id)) {
                return ok(false).boxify();
            }
        }

        self.inner
            .get(
                ctx,
                repo_id,
                BonsaiOrHgChangesetIds::Bonsai(vec![entry.bcs_id]),
            )
            .and_then({
                cloned!(self.mappings);
                move |maybe_mapping| {
                    if maybe_mapping.is_empty() {
                        let mut mappings = mappings.lock().expect("lock poisoned");
                        {
                            let hg_to_bcs = &mut mappings.hg_to_bcs;
                            hg_to_bcs.insert((repo_id, entry.hg_cs_id), entry.bcs_id);
                        }
                        {
                            let bcs_to_hg = &mut mappings.bcs_to_hg;
                            bcs_to_hg.insert((repo_id, entry.bcs_id), entry.hg_cs_id);
                        }
                        {
                            let ordered_inserts = &mut mappings.ordered_inserts;
                            ordered_inserts.push(entry);
                        }

                        Ok(true)
                    } else {
                        Ok(false)
                    }
                }
            })
            .boxify()
    }

    fn get(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        ids: BonsaiOrHgChangesetIds,
    ) -> BoxFuture<Vec<BonsaiHgMappingEntry>, Error> {
        let (mut mappings, left_to_fetch) = {
            let mappings = self.mappings.lock().expect("lock poisoned");

            let hg_to_bcs = &mappings.hg_to_bcs;
            let bcs_to_hg = &mappings.bcs_to_hg;

            let mappings: Vec<(HgChangesetId, ChangesetId)> = match ids {
                BonsaiOrHgChangesetIds::Bonsai(ref bcs_id) => bcs_id
                    .into_iter()
                    .filter_map(|bcs_id| {
                        bcs_to_hg
                            .get(&(repo_id, *bcs_id))
                            .map(move |hg_cs_id| (*hg_cs_id, *bcs_id))
                    })
                    .collect(),
                BonsaiOrHgChangesetIds::Hg(ref hg_cs_id) => hg_cs_id
                    .into_iter()
                    .filter_map(|hg_cs_id| {
                        hg_to_bcs
                            .get(&(repo_id, *hg_cs_id))
                            .map(move |bcs_id| (*hg_cs_id, *bcs_id))
                    })
                    .collect(),
            };

            let mappings: Vec<_> = mappings
                .into_iter()
                .map(|(hg_cs_id, bcs_id)| BonsaiHgMappingEntry::new(repo_id, hg_cs_id, bcs_id))
                .collect();

            let filtered_ids = filter_fetched_ids(ids, &mappings[..]);
            (mappings, filtered_ids)
        };

        if left_to_fetch.is_empty() {
            ok(mappings).boxify()
        } else {
            self.inner
                .get(ctx, repo_id, left_to_fetch)
                .map(move |mut mappings_from_inner| {
                    mappings.append(&mut mappings_from_inner);
                    mappings
                })
                .boxify()
        }
    }
}
