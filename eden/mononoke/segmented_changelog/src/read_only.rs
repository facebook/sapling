/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::Arc;

use anyhow::bail;
use anyhow::format_err;
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use futures::prelude::*;

use stats::prelude::*;

use context::CoreContext;
use mercurial_types::HgChangesetId;
use mononoke_types::ChangesetId;

use crate::idmap::IdMap;
use crate::CloneData;
use crate::DagId;
use crate::DagIdSet;
use crate::FirstAncestorConstraint;
use crate::Group;
use crate::InProcessIdDag;
use crate::Location;
use crate::SegmentedChangelog;

const IDMAP_CHANGESET_FETCH_BATCH: usize = 500;

define_stats! {
    prefix = "mononoke.segmented_changelog.read_only";
    location_to_changeset_id: timeseries(Sum),
}

pub struct ReadOnlySegmentedChangelog<'a> {
    pub(crate) iddag: &'a InProcessIdDag,
    pub(crate) idmap: Arc<dyn IdMap>,
}

#[async_trait]
impl<'a> SegmentedChangelog for ReadOnlySegmentedChangelog<'a> {
    async fn location_to_many_changeset_ids(
        &self,
        ctx: &CoreContext,
        location: Location<ChangesetId>,
        count: u64,
    ) -> Result<Vec<ChangesetId>> {
        STATS::location_to_changeset_id.add_value(1);
        let location = location
            .and_then_descendant(|hgid| self.idmap.get_dag_id(ctx, hgid))
            .await?;
        self.known_location_to_many_changeset_ids(ctx, location, count)
            .await
    }

    async fn many_changeset_ids_to_locations(
        &self,
        ctx: &CoreContext,
        master_heads: Vec<ChangesetId>,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Result<Location<ChangesetId>>>> {
        let (master_head_dag_ids, cs_to_dag_id) = futures::try_join!(
            self.idmap.find_many_dag_ids(ctx, master_heads.clone()),
            self.idmap.find_many_dag_ids(ctx, cs_ids),
        )
        .context("failed fetching changeset to dag_id translations")?;
        if master_head_dag_ids.is_empty() {
            // When the client has multiple heads, we are content with the server finding only one
            // of the heads. This situation comes up when master moves backwards.  The server may
            // be reseeded after that and will not have multiple heads. The client then may have
            // multiple heads and we will have to treat the heads that are not found as non master
            // heads.
            bail!(
                "failed to find idmap entries for all commits listed in \
                the master heads list: {:?}",
                master_heads
            );
        }
        let constraints = FirstAncestorConstraint::KnownUniversally {
            heads: DagIdSet::from_spans(master_head_dag_ids.into_iter().map(|(_k, v)| v)),
        };
        let cs_to_vlocation: HashMap<ChangesetId, Result<Option<Location<_>>>> = cs_to_dag_id
            .into_iter()
            .map(|(cs_id, dag_id)| {
                let result = self
                    .iddag
                    .to_first_ancestor_nth(dag_id, constraints.clone());
                let cs_id_result = match result
                {
                    // Preserve error message in server response by flatten the error.
                    Err(e) => Err(format_err!(
                        "failed to compute the common descendant and distance for {} with heads {:?}: {:?}",
                        cs_id,
                        &master_heads,
                        e
                    )),
                    Ok(Some((v, dist))) => Ok(Some(Location::new(v, dist))),
                    Ok(None) => Ok(None),
                };
                (cs_id, cs_id_result)
            })
            .collect();
        let common_cs_ids = {
            let to_fetch = cs_to_vlocation
                .values()
                .filter_map(|l| match l {
                    Ok(Some(l)) => Some(l.descendant),
                    _ => None,
                })
                .collect();
            self.idmap
                .find_many_changeset_ids(ctx, to_fetch)
                .await
                .context("failed fetching dag_id to changeset translations")?
        };
        let locations: HashMap<ChangesetId, Result<Location<_>>> = cs_to_vlocation
            .into_iter()
            .filter_map(|(cs, cs_result)| {
                let cs_result = match cs_result {
                    Ok(Some(location)) => Some(location.try_map_descendant(|dag_id| {
                        common_cs_ids.get(&dag_id).cloned().ok_or_else(|| {
                            format_err!("failed to find dag_id translation for {}", dag_id)
                        })
                    })),
                    Ok(None) => None,
                    Err(e) => Some(Err(e)),
                };
                cs_result.map(|r| (cs, r))
            })
            .collect();
        Ok(locations)
    }

    async fn clone_data(
        &self,
        ctx: &CoreContext,
    ) -> Result<(CloneData<ChangesetId>, HashMap<ChangesetId, HgChangesetId>)> {
        self.clone_data_with_hints(ctx, HashMap::new()).await
    }

    async fn pull_data(
        &self,
        ctx: &CoreContext,
        common: Vec<ChangesetId>,
        missing: Vec<ChangesetId>,
    ) -> Result<CloneData<ChangesetId>> {
        let all_cs_ids: Vec<_> = common.iter().chain(missing.iter()).cloned().collect();
        let request_ids = self
            .idmap
            .find_many_dag_ids(ctx, all_cs_ids.clone())
            .await?;

        let common_ids = common
            .iter()
            .map(|i| {
                request_ids
                    .get(i)
                    .copied()
                    .ok_or_else(|| format_err!("common head {} not found", i))
            })
            .collect::<Result<Vec<_>>>()?;
        let missing_ids = missing
            .iter()
            .map(|i| {
                request_ids
                    .get(i)
                    .copied()
                    .ok_or_else(|| format_err!("missing head {} not found", i))
            })
            .collect::<Result<Vec<_>>>()?;

        let common_id_set = DagIdSet::from_spans(common_ids.into_iter());
        let missing_id_set = DagIdSet::from_spans(missing_ids.into_iter());
        let common_ancestors_id_set = self.iddag.ancestors(common_id_set)?;
        let missing_ancestors_id_set = self.iddag.ancestors(missing_id_set)?;

        let missing_id_set = missing_ancestors_id_set.difference(&common_ancestors_id_set);
        let flat_segments = self.iddag.idset_to_flat_segments(missing_id_set)?;

        let ids = flat_segments.parents_head_and_roots().into_iter().collect();

        let idmap = self
            .idmap
            .find_many_changeset_ids(ctx, ids)
            .await
            .context("error retrieving mappings for parents_head_and_roots")?;

        let pull_data = CloneData {
            flat_segments,
            idmap: idmap.into_iter().collect(),
        };
        Ok(pull_data)
    }

    /// Test if `ancestor` is an ancestor of `descendant`.
    /// Returns None in case segmented changelog doesn't know about either of those commit.
    async fn is_ancestor(
        &self,
        ctx: &CoreContext,
        ancestor: ChangesetId,
        descendant: ChangesetId,
    ) -> Result<Option<bool>> {
        let request_ids = self
            .idmap
            .find_many_dag_ids_maybe_stale(ctx, vec![ancestor, descendant])
            .await?;
        let ancestor_id = if let Some(ancestor_id) = request_ids.get(&ancestor) {
            ancestor_id
        } else {
            return Ok(None);
        };
        let descendant_id = if let Some(descendant_id) = request_ids.get(&descendant) {
            descendant_id
        } else {
            return Ok(None);
        };

        // Even though the ids exist, our local DAG might not have them.
        let all = self.iddag.all()?;
        if !all.contains(descendant_id.clone()) || !all.contains(ancestor_id.clone()) {
            return Ok(None);
        }

        Ok(Some(self.iddag.is_ancestor(*ancestor_id, *descendant_id)?))
    }

    async fn disabled(&self, _ctx: &CoreContext) -> Result<bool> {
        Ok(false)
    }
}

impl<'a> ReadOnlySegmentedChangelog<'a> {
    pub fn new(iddag: &'a InProcessIdDag, idmap: Arc<dyn IdMap>) -> Self {
        Self { iddag, idmap }
    }

    pub(crate) async fn known_location_to_many_changeset_ids(
        &self,
        ctx: &CoreContext,
        location: Location<DagId>,
        count: u64,
    ) -> Result<Vec<ChangesetId>> {
        STATS::location_to_changeset_id.add_value(1);
        let mut dist_ancestor_dag_id = self
            .iddag
            .first_ancestor_nth(location.descendant, location.distance)
            .with_context(|| format!("failed to compute location origin for {:?}", location))?;
        let mut dag_ids = vec![dist_ancestor_dag_id];
        for _ in 1..count {
            let parents = self
                .iddag
                .parent_ids(dist_ancestor_dag_id)
                .with_context(|| format!("looking up parents ids for {}", dist_ancestor_dag_id))?;
            if parents.len() != 1 {
                return Err(format_err!(
                    "invalid request: changeset with dag_id {} does not have {} single parent ancestors",
                    location.descendant,
                    location.distance + count - 1
                ));
            }
            dist_ancestor_dag_id = parents[0];
            dag_ids.push(dist_ancestor_dag_id);
        }
        let changeset_futures = dag_ids
            .into_iter()
            .map(|dag_id| self.idmap.get_changeset_id(ctx, dag_id));
        stream::iter(changeset_futures)
            .buffered(IDMAP_CHANGESET_FETCH_BATCH)
            .try_collect()
            .await
    }

    pub(crate) async fn clone_data_with_hints(
        &self,
        ctx: &CoreContext,
        mut hints: HashMap<DagId, (ChangesetId, HgChangesetId)>,
    ) -> Result<(CloneData<ChangesetId>, HashMap<ChangesetId, HgChangesetId>)> {
        let group = Group::MASTER;
        let flat_segments = self
            .iddag
            .flat_segments(group)
            .context("error during flat segment retrieval")?;
        let (idmap, hints) = {
            let universal_ids: Vec<_> = self
                .iddag
                .universal_ids()
                .context("error computing universal ids")?
                .into_iter()
                .collect();
            let mut to_fetch: Vec<_> =
                Vec::with_capacity(universal_ids.len().saturating_sub(hints.len()));

            let mut idmap = BTreeMap::new();
            let mut output_hints = HashMap::with_capacity(hints.len());

            for id in universal_ids {
                if let Some((cs, hgcs)) = hints.remove(&id) {
                    idmap.insert(id, cs);
                    output_hints.insert(cs, hgcs);
                } else {
                    to_fetch.push(id);
                }
            }

            if !to_fetch.is_empty() {
                let fetched_idmap = self
                    .idmap
                    .find_many_changeset_ids(ctx, to_fetch)
                    .await
                    .context("error retrieving mappings for dag universal ids")?;
                idmap.extend(fetched_idmap);
            }

            (idmap, output_hints)
        };
        let clone_data = CloneData {
            flat_segments,
            idmap,
        };
        Ok((clone_data, hints))
    }
}
