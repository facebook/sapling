/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;

use bonsai_git_mapping::BonsaiGitMappingRef;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use commit_graph::ChangesetSegment as CommitGraphSegment;
use commit_graph::CommitGraphRef;
use dag_types::Location;
use futures::future;
use futures::stream;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use mercurial_types::HgChangesetId;
use mononoke_types::ChangesetId;
use mononoke_types::hash::GitSha1;

use super::MononokeRepo;
use super::RepoContext;
use crate::MononokeError;

pub struct ChangesetSegment<Id> {
    pub head: Id,
    pub base: Id,
    pub length: u64,
    pub parents: Vec<ChangesetSegmentParent<Id>>,
}

impl<Id> ChangesetSegment<Id> {
    pub fn map_ids<NewId>(self, map_id: impl Fn(Id) -> NewId + Clone) -> ChangesetSegment<NewId> {
        ChangesetSegment {
            head: map_id(self.head),
            base: map_id(self.base),
            length: self.length,
            parents: self
                .parents
                .into_iter()
                .map(|parent| parent.map_ids(map_id.clone()))
                .collect(),
        }
    }
}

pub struct ChangesetSegmentParent<Id> {
    pub id: Id,
    pub location: Option<Location<Id>>,
}

impl<Id> ChangesetSegmentParent<Id> {
    pub fn map_ids<NewId>(self, map_id: impl Fn(Id) -> NewId) -> ChangesetSegmentParent<NewId> {
        ChangesetSegmentParent {
            id: map_id(self.id),
            location: self
                .location
                .map(|location| location.map_descendant(map_id)),
        }
    }
}

impl<R: MononokeRepo> RepoContext<R> {
    fn make_graph_segments_stream<Id: Copy + 'static>(
        &self,
        segments: Vec<CommitGraphSegment>,
        mapping: HashMap<ChangesetId, Id>,
    ) -> impl Stream<Item = Result<ChangesetSegment<Id>, MononokeError>> + '_ {
        let map_id = move |name, csid| {
            mapping
                .get(&csid)
                .ok_or_else(|| {
                    MononokeError::InvalidRequest(format!(
                        "failed to find mapped commit for {} {}",
                        name, csid,
                    ))
                })
                .copied()
        };
        stream::iter(segments.into_iter().map(move |segment| {
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
        }))
    }

    /// Get a stream of the linear segments of the commit graph between the common and heads as HgChangesetIds.
    pub async fn graph_segments_hg(
        &self,
        common: Vec<HgChangesetId>,
        heads: Vec<HgChangesetId>,
    ) -> Result<
        impl Stream<Item = Result<ChangesetSegment<HgChangesetId>, MononokeError>> + '_,
        MononokeError,
    > {
        let (bonsai_common, bonsai_heads) = future::try_join(
            self.repo()
                .bonsai_hg_mapping()
                .convert_all_hg_to_bonsai(self.ctx(), common),
            self.repo()
                .bonsai_hg_mapping()
                .convert_all_hg_to_bonsai(self.ctx(), heads),
        )
        .await?;

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
                    ids.extend(segment.ids())
                }
                let mapping = self
                    .repo()
                    .bonsai_hg_mapping()
                    .get_bonsai_to_hg_map(self.ctx(), ids.into_iter().collect::<Vec<_>>())
                    .await?;
                anyhow::Ok(self.make_graph_segments_stream(segments, mapping))
            })
            .buffered(25)
            .try_flatten())
    }

    /// Get a stream of the linear segments of the commit graph between the common and heads as GitSha1.
    pub async fn graph_segments_git(
        &self,
        common: Vec<GitSha1>,
        heads: Vec<GitSha1>,
    ) -> Result<
        impl Stream<Item = Result<ChangesetSegment<GitSha1>, MononokeError>> + '_,
        MononokeError,
    > {
        let (bonsai_common, bonsai_heads) = future::try_join(
            self.repo()
                .bonsai_git_mapping()
                .convert_all_git_to_bonsai(self.ctx(), common),
            self.repo()
                .bonsai_git_mapping()
                .convert_all_git_to_bonsai(self.ctx(), heads),
        )
        .await?;

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
                    ids.extend(segment.ids());
                }
                let mapping: HashMap<ChangesetId, GitSha1> = self
                    .repo()
                    .bonsai_git_mapping()
                    .get_bonsai_to_git_map(self.ctx(), ids.into_iter().collect::<Vec<_>>())
                    .await?;
                anyhow::Ok(self.make_graph_segments_stream(segments, mapping))
            })
            .buffered(25)
            .try_flatten())
    }
}
