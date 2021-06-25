/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{bail, format_err, Context, Result};
use async_trait::async_trait;
use futures::prelude::*;

use cloned::cloned;
use stats::prelude::*;

use context::CoreContext;
use mononoke_types::ChangesetId;

use crate::idmap::IdMap;
use crate::{CloneData, DagId, DagIdSet, FirstAncestorConstraint, Group, InProcessIdDag, Location};
use crate::{SegmentedChangelog, StreamCloneData};

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
    ) -> Result<HashMap<ChangesetId, Location<ChangesetId>>> {
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
        let cs_to_vlocation = cs_to_dag_id
            .into_iter()
            .filter_map(|(cs_id, dag_id)| {
                // We do not return an entry when the asked commit is a descendant of client_head.
                self.iddag
                    .to_first_ancestor_nth(dag_id, constraints.clone())
                    .with_context(|| {
                        format!(
                            "failed to compute the common descendant and distance for {}",
                            cs_id
                        )
                    })
                    .map(|opt| opt.map(|(v, dist)| (cs_id, Location::new(v, dist))))
                    .transpose()
            })
            .collect::<Result<HashMap<_, _>>>()?;
        let common_cs_ids = {
            let to_fetch = cs_to_vlocation.values().map(|l| l.descendant).collect();
            self.idmap
                .find_many_changeset_ids(ctx, to_fetch)
                .await
                .context("failed fetching dag_id to changeset translations")?
        };
        let locations = cs_to_vlocation
            .into_iter()
            .map(|(cs, location)| {
                location
                    .try_map_descendant(|dag_id| {
                        common_cs_ids.get(&dag_id).cloned().ok_or_else(|| {
                            format_err!("failed to find dag_id translation for {}", dag_id)
                        })
                    })
                    .map(|l| (cs, l))
            })
            .collect::<Result<HashMap<_, _>>>()?;
        Ok(locations)
    }

    async fn clone_data(&self, ctx: &CoreContext) -> Result<CloneData<ChangesetId>> {
        let group = Group::MASTER;
        let flat_segments = self
            .iddag
            .flat_segments(group)
            .context("error during flat segment retrieval")?;
        let universal_ids = self
            .iddag
            .universal_ids()
            .context("error computing universal ids")?
            .into_iter()
            .collect();
        let idmap = self
            .idmap
            .find_many_changeset_ids(&ctx, universal_ids)
            .await
            .context("error retrieving mappings for dag universal ids")?;
        let clone_data = CloneData {
            flat_segments,
            idmap,
        };
        Ok(clone_data)
    }

    async fn pull_fast_forward_master(
        &self,
        ctx: &CoreContext,
        old_master: ChangesetId,
        new_master: ChangesetId,
    ) -> Result<CloneData<ChangesetId>> {
        let request_ids = self
            .idmap
            .find_many_dag_ids(ctx, vec![old_master, new_master])
            .await?;
        let old = *request_ids
            .get(&old_master)
            .ok_or_else(|| format_err!("Old id {} not found", old_master))?;
        let new = *request_ids
            .get(&new_master)
            .ok_or_else(|| format_err!("New id {} not found", new_master))?;
        let master_group = self.iddag.master_group()?;

        if !master_group.contains(old) {
            bail!("old vertex {} is not in master group", old);
        }

        if !master_group.contains(new) {
            bail!("new vertex {} is not in master group", new);
        }
        let old_ancestors = self.iddag.ancestors(old.into())?;
        let new_ancestors = self.iddag.ancestors(new.into())?;

        let result_span = new_ancestors.difference(&old_ancestors);
        let flat_segments = self.iddag.idset_to_flat_segments(result_span)?;

        let ids = flat_segments.parents_head_and_roots().into_iter().collect();

        let idmap = self
            .idmap
            .find_many_changeset_ids(&ctx, ids)
            .await
            .context("error retrieving mappings for parents_head_and_roots")?;

        let pull_data = CloneData {
            flat_segments,
            idmap,
        };
        Ok(pull_data)
    }

    async fn full_idmap_clone_data(
        &self,
        ctx: &CoreContext,
    ) -> Result<StreamCloneData<ChangesetId>> {
        const CHUNK_SIZE: usize = 1000;
        const BUFFERED_BATCHES: usize = 5;
        let group = Group::MASTER;
        let next_id = {
            let group = Group::MASTER;
            let level = 0;
            self.iddag
                .next_free_id(level, group)
                .context("error computing next free id for dag")?
        };
        let flat_segments = self
            .iddag
            .flat_segments(group)
            .context("error during flat segment retrieval")?;
        let idmap_stream = stream::iter((group.min_id().0..next_id.0).into_iter().map(DagId))
            .chunks(CHUNK_SIZE)
            .map({
                cloned!(ctx, self.idmap);
                move |chunk| {
                    cloned!(ctx, idmap);
                    async move { idmap.find_many_changeset_ids(&ctx, chunk).await }
                }
            })
            .buffered(BUFFERED_BATCHES)
            .map_ok(|map_chunk| stream::iter(map_chunk.into_iter().map(Ok)))
            .try_flatten()
            .boxed();
        let stream_clone_data = StreamCloneData {
            flat_segments,
            idmap_stream,
        };
        Ok(stream_clone_data)
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
}
