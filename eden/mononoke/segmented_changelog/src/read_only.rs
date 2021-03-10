/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{format_err, Context, Result};
use async_trait::async_trait;
use futures::prelude::*;

use cloned::cloned;
use dag::{
    self, CloneData, FirstAncestorConstraint, Group, Id as Vertex, InProcessIdDag, Location,
};
use stats::prelude::*;

use context::CoreContext;
use mononoke_types::ChangesetId;

use crate::idmap::IdMap;
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
            .and_then_descendant(|hgid| self.idmap.get_vertex(ctx, hgid))
            .await?;
        self.known_location_to_many_changeset_ids(ctx, location, count)
            .await
    }

    async fn many_changeset_ids_to_locations(
        &self,
        ctx: &CoreContext,
        client_head: ChangesetId,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Location<ChangesetId>>> {
        let (head_vertex, cs_to_vertex) = futures::try_join!(
            self.idmap.get_vertex(ctx, client_head),
            self.idmap.find_many_vertexes(ctx, cs_ids),
        )
        .context("failed fetching changeset to vertex translations")?;
        let constraints = FirstAncestorConstraint::KnownUniversally {
            heads: head_vertex.into(),
        };
        let cs_to_vlocation = cs_to_vertex
            .into_iter()
            .filter_map(|(cs_id, vertex)| {
                // We do not return an entry when the asked commit is a descendant of client_head.
                self.iddag
                    .to_first_ancestor_nth(vertex, constraints.clone())
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
                .context("failed fetching vertex to changeset translations")?
        };
        let locations = cs_to_vlocation
            .into_iter()
            .map(|(cs, location)| {
                location
                    .try_map_descendant(|vertex| {
                        common_cs_ids.get(&vertex).cloned().ok_or_else(|| {
                            format_err!("failed to find vertex translation for {}", vertex)
                        })
                    })
                    .map(|l| (cs, l))
            })
            .collect::<Result<HashMap<_, _>>>()?;
        Ok(locations)
    }

    async fn clone_data(&self, ctx: &CoreContext) -> Result<CloneData<ChangesetId>> {
        let group = Group::MASTER;
        let head_id = self.clone_data_head_id()?;
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
            head_id,
            flat_segments,
            idmap,
        };
        Ok(clone_data)
    }

    async fn full_idmap_clone_data(
        &self,
        ctx: &CoreContext,
    ) -> Result<StreamCloneData<ChangesetId>> {
        const CHUNK_SIZE: usize = 1000;
        const BUFFERED_BATCHES: usize = 5;
        let group = Group::MASTER;
        let head_id = self.clone_data_head_id()?;
        let flat_segments = self
            .iddag
            .flat_segments(group)
            .context("error during flat segment retrieval")?;
        let idmap_stream = stream::iter((group.min_id().0..=head_id.0).into_iter().map(Vertex))
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
            head_id,
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
        location: Location<Vertex>,
        count: u64,
    ) -> Result<Vec<ChangesetId>> {
        STATS::location_to_changeset_id.add_value(1);
        let mut dist_ancestor_vertex = self
            .iddag
            .first_ancestor_nth(location.descendant, location.distance)
            .with_context(|| format!("failed to compute location origin for {:?}", location))?;
        let mut vertexes = vec![dist_ancestor_vertex];
        for _ in 1..count {
            let parents = self
                .iddag
                .parent_ids(dist_ancestor_vertex)
                .with_context(|| format!("looking up parents ids for {}", dist_ancestor_vertex))?;
            if parents.len() != 1 {
                return Err(format_err!(
                    "invalid request: changeset with vertex {} does not have {} single parent ancestors",
                    location.descendant,
                    location.distance + count - 1
                ));
            }
            dist_ancestor_vertex = parents[0];
            vertexes.push(dist_ancestor_vertex);
        }
        let changeset_futures = vertexes
            .into_iter()
            .map(|vertex| self.idmap.get_changeset_id(ctx, vertex));
        stream::iter(changeset_futures)
            .buffered(IDMAP_CHANGESET_FETCH_BATCH)
            .try_collect()
            .await
    }

    fn clone_data_head_id(&self) -> Result<Vertex> {
        let group = Group::MASTER;
        let level = 0;
        let next_id = self
            .iddag
            .next_free_id(level, group)
            .context("error computing next free id for dag")?;
        if next_id > group.min_id() {
            Ok(next_id - 1)
        } else {
            Err(format_err!("error generating clone data for empty iddag"))
        }
    }
}
