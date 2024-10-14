/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;

use anyhow::Context;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use commit_graph::CommitGraphRef;
use dag_types::Location;
use futures::stream;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use mercurial_types::HgChangesetId;
use mononoke_types::ChangesetId;

use super::MononokeRepo;
use super::RepoContext;
use crate::MononokeError;

pub struct ChangesetSegment<Id> {
    pub head: Id,
    pub base: Id,
    pub length: u64,
    pub parents: Vec<ChangesetSegmentParent<Id>>,
}

pub struct ChangesetSegmentParent<Id> {
    pub id: Id,
    pub location: Option<Location<Id>>,
}

impl<R: MononokeRepo> RepoContext<R> {
    /// Get a stream of the linear segments of the commit graph between the common and heads as HgChangesetIds.
    pub async fn graph_segments_hg(
        &self,
        common: Vec<HgChangesetId>,
        heads: Vec<HgChangesetId>,
    ) -> Result<
        impl Stream<Item = Result<ChangesetSegment<HgChangesetId>, MononokeError>> + '_,
        MononokeError,
    > {
        let bonsai_common = self
            .repo()
            .bonsai_hg_mapping()
            .get(self.ctx(), common.into())
            .await?
            .into_iter()
            .map(|e| e.bcs_id)
            .collect();
        let bonsai_heads = self
            .repo()
            .bonsai_hg_mapping()
            .get(self.ctx(), heads.into())
            .await?
            .into_iter()
            .map(|e| e.bcs_id)
            .collect();

        let segments = self
            .repo()
            .commit_graph()
            .ancestors_difference_segments(self.ctx(), bonsai_heads, bonsai_common)
            .await?;

        Ok(stream::iter(segments.into_iter())
            .chunks(25)
            .map(move |segments| async move {
                let mut ids = HashSet::with_capacity(segments.len() * 4);
                for segment in segments.iter() {
                    ids.insert(segment.head);
                    ids.insert(segment.base);
                    for parent in segment.parents.iter() {
                        ids.insert(parent.cs_id);
                        if let Some(location) = &parent.location {
                            ids.insert(location.head);
                        }
                    }
                }
                let mapping: HashMap<ChangesetId, HgChangesetId> = self
                    .repo()
                    .bonsai_hg_mapping()
                    .get(self.ctx(), ids.into_iter().collect::<Vec<_>>().into())
                    .await
                    .context("error fetching bonsai-hg mapping")?
                    .into_iter()
                    .map(|e| (e.bcs_id, e.hg_cs_id))
                    .collect();
                let map_id = move |name, csid| {
                    mapping
                        .get(&csid)
                        .ok_or_else(|| {
                            MononokeError::InvalidRequest(format!(
                                "failed to find hg equivalent for {} {}",
                                name, csid,
                            ))
                        })
                        .copied()
                };
                anyhow::Ok(stream::iter(segments.into_iter().map(move |segment| {
                    Ok(ChangesetSegment {
                        head: map_id("segment head", segment.head)?,
                        base: map_id("segment base", segment.base)?,
                        length: segment.length,
                        parents: segment
                            .parents
                            .into_iter()
                            .map(|parent| {
                                Ok(ChangesetSegmentParent {
                                    id: map_id("segment parent", parent.cs_id)?,
                                    location: parent
                                        .location
                                        .map(|location| {
                                            anyhow::Ok(Location::new(
                                                map_id("location head", location.head)?,
                                                location.distance,
                                            ))
                                        })
                                        .transpose()?,
                                })
                            })
                            .collect::<Result<_, MononokeError>>()?,
                    })
                })))
            })
            .buffered(25)
            .try_flatten())
    }
}
