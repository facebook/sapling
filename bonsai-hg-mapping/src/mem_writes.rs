// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use futures::{Future, IntoFuture};
use futures_ext::{BoxFuture, FutureExt};
use mercurial_types::{HgChangesetId, RepositoryId};
use mononoke_types::ChangesetId;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::{BonsaiHgMapping, BonsaiHgMappingEntry, BonsaiOrHgChangesetId, errors::Error};

/// A bonsai <-> hg mapping wrapper that reads from the underlying mapping but writes to memory.
/// It can be used to prevent a piece of code to do writes to the db but at the same time give
/// this piece of code a consistent view of the repository.
#[derive(Clone)]
pub struct MemWritesBonsaiHgMapping {
    mappings: Arc<Mutex<InMemoryMappings>>,

    inner: Arc<BonsaiHgMapping>,
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
    pub fn new(inner: Arc<BonsaiHgMapping>) -> Self {
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

    pub fn get_inner(&self) -> Arc<BonsaiHgMapping> {
        self.inner.clone()
    }
}

impl BonsaiHgMapping for MemWritesBonsaiHgMapping {
    fn add(&self, entry: BonsaiHgMappingEntry) -> BoxFuture<bool, Error> {
        let repo_id = entry.repo_id;
        {
            let mappings = self.mappings.lock().expect("lock poisoned");

            let bcs_to_hg = &mappings.bcs_to_hg;

            if bcs_to_hg.contains_key(&(repo_id, entry.bcs_id)) {
                return Ok(false).into_future().boxify();
            }
        }

        self.inner
            .get(repo_id, BonsaiOrHgChangesetId::Bonsai(entry.bcs_id))
            .and_then({
                let mappings = self.mappings.clone();
                move |maybe_mapping| match maybe_mapping {
                    Some(_) => Ok(false),
                    None => {
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
                    }
                }
            })
            .boxify()
    }

    fn get(
        &self,
        repo_id: RepositoryId,
        cs_id: BonsaiOrHgChangesetId,
    ) -> BoxFuture<Option<BonsaiHgMappingEntry>, Error> {
        {
            let mappings = self.mappings.lock().expect("lock poisoned");

            let hg_to_bcs = &mappings.hg_to_bcs;
            let bcs_to_hg = &mappings.bcs_to_hg;

            match cs_id {
                BonsaiOrHgChangesetId::Bonsai(bcs_id) => {
                    if let Some(hg_cs_id) = bcs_to_hg.get(&(repo_id, bcs_id)) {
                        return Ok(Some(BonsaiHgMappingEntry::new(repo_id, *hg_cs_id, bcs_id)))
                            .into_future()
                            .boxify();
                    }
                }
                BonsaiOrHgChangesetId::Hg(hg_cs_id) => {
                    if let Some(bcs_id) = hg_to_bcs.get(&(repo_id, hg_cs_id)) {
                        return Ok(Some(BonsaiHgMappingEntry::new(repo_id, hg_cs_id, *bcs_id)))
                            .into_future()
                            .boxify();
                    }
                }
            };
        }

        self.inner.get(repo_id, cs_id)
    }
}
