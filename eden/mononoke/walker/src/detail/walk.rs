/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Debug;
use std::sync::Arc;

use anyhow::format_err;
use anyhow::Context;
use anyhow::Error;
use async_trait::async_trait;
use auto_impl::auto_impl;
use basename_suffix_skeleton_manifest::RootBasenameSuffixSkeletonManifest;
use blame::BlameRoot;
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use blobstore::LoadableError;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_hg_mapping::BonsaiHgMappingArc;
use bonsai_hg_mapping::BonsaiHgMappingEntry;
use bookmarks::BookmarkKind;
use bookmarks::BookmarkName;
use bookmarks::BookmarkPagination;
use bookmarks::BookmarkPrefix;
use bookmarks::Freshness;
use bounded_traversal::limited_by_key_shardable;
use changeset_info::ChangesetInfo;
use cloned::cloned;
use context::CoreContext;
use deleted_manifest::RootDeletedManifestIdCommon;
use deleted_manifest::RootDeletedManifestV2Id;
use derived_data::BonsaiDerived;
use derived_data_filenodes::FilenodesOnlyPublic;
use derived_data_manager::BonsaiDerivable as NewBonsaiDerivable;
use fastlog::fetch_fastlog_batch_by_unode_id;
use fastlog::RootFastlog;
use filenodes::FilenodeInfo;
use filestore::Alias;
use fsnodes::RootFsnodeId;
use futures::future;
use futures::future::FutureExt;
use futures::future::TryFutureExt;
use futures::stream;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use manifest::AsyncManifest;
use manifest::Entry;
use mercurial_derived_data::MappedHgChangesetId;
use mercurial_types::FileBytes;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mercurial_types::RepoPath;
use mononoke_types::blame::BlameMaybeRejected;
use mononoke_types::deleted_manifest_common::DeletedManifestCommon;
use mononoke_types::fsnode::FsnodeEntry;
use mononoke_types::skeleton_manifest::SkeletonManifestEntry;
use mononoke_types::unode::UnodeEntry;
use mononoke_types::BasenameSuffixSkeletonManifestId;
use mononoke_types::BlameId;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::DeletedManifestV2Id;
use mononoke_types::FastlogBatchId;
use mononoke_types::FileUnodeId;
use mononoke_types::FsnodeId;
use mononoke_types::MPath;
use mononoke_types::ManifestUnodeId;
use mononoke_types::SkeletonManifestId;
use phases::Phase;
use phases::Phases;
use phases::PhasesRef;
use scuba_ext::MononokeScubaSampleBuilder;
use skeleton_manifest::RootSkeletonManifestId;
use slog::info;
use slog::warn;
use slog::Logger;
use thiserror::Error;
use unodes::RootUnodeManifestId;
use yield_stream::YieldStreamExt;

use crate::commands::JobWalkParams;
use crate::detail::graph::AliasKey;
use crate::detail::graph::ChangesetKey;
use crate::detail::graph::EdgeType;
use crate::detail::graph::FastlogKey;
use crate::detail::graph::FileContentData;
use crate::detail::graph::HashValidationError;
use crate::detail::graph::Node;
use crate::detail::graph::NodeData;
use crate::detail::graph::NodeType;
use crate::detail::graph::PathKey;
use crate::detail::graph::SqlShardInfo;
use crate::detail::graph::UnodeFlags;
use crate::detail::graph::UnodeKey;
use crate::detail::graph::UnodeManifestEntry;
use crate::detail::graph::WrappedPath;
use crate::detail::log;
use crate::detail::state::InternedType;
use crate::detail::validate::add_node_to_scuba;
use crate::detail::validate::CHECK_FAIL;
use crate::detail::validate::CHECK_TYPE;
use crate::detail::validate::EDGE_TYPE;
use crate::detail::validate::ERROR_MSG;

/// How frequently to yield the CPU when processing large manifests.
const MANIFEST_YIELD_EVERY_ENTRY_COUNT: usize = 10_000;

pub trait StepRoute: Debug {
    /// Where we stepped from, useful for immediate reproductions with --walk-root
    fn source_node(&self) -> Option<&Node>;

    /// What the check thinks is an interesting node on the route to here (e.g. the affected changeset)
    fn via_node(&self) -> Option<&Node>;
}

#[derive(Clone, Debug)]
pub struct EmptyRoute();
// No useful node info held.
impl StepRoute for EmptyRoute {
    fn source_node(&self) -> Option<&Node> {
        None
    }
    fn via_node(&self) -> Option<&Node> {
        None
    }
}

// Holds type of edge and target Node that we want to load in next step(s)
// Combined with current node, this forms an complegte edge.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OutgoingEdge {
    pub label: EdgeType,
    pub target: Node,
    pub path: Option<WrappedPath>,
}

impl OutgoingEdge {
    pub fn new(label: EdgeType, target: Node) -> Self {
        Self {
            label,
            target,
            path: None,
        }
    }

    pub fn new_with_path(label: EdgeType, target: Node, path: Option<WrappedPath>) -> Self {
        Self {
            label,
            target,
            path,
        }
    }
}

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Could not step to {1:?} via {2} in repo {0}")]
    NotTraversable(String, OutgoingEdge, String),
}

// Simpler visitor trait used inside each step to decide
// whether to emit an edge
#[async_trait]
#[auto_impl(Arc)]
pub trait VisitOne {
    fn in_chunk(&self, bcs_id: &ChangesetId) -> bool;

    fn needs_visit(&self, outgoing: &OutgoingEdge) -> bool;

    async fn is_public(
        &self,
        ctx: &CoreContext,
        phases_store: &dyn Phases,
        bcs_id: &ChangesetId,
    ) -> Result<bool, Error>;

    /// This only checks if its in the state, it doesn't load it from storage as unlike get_bonsai_from_hg it might require derivation
    fn get_hg_from_bonsai(&self, bcs_id: &ChangesetId) -> Option<HgChangesetId>;

    /// Record the derived HgChangesetId in the visitor state if we know it
    fn record_hg_from_bonsai(&self, bcs_id: &ChangesetId, hg_cs_id: HgChangesetId);

    /// Gets the (possibly preloaded) hg to bonsai mapping
    async fn get_bonsai_from_hg(
        &self,
        ctx: &CoreContext,
        bonsai_hg_mapping: &dyn BonsaiHgMapping,
        hg_cs_id: &HgChangesetId,
    ) -> Result<ChangesetId, Error>;

    /// returns ChangesetId to defer with if deferral is needed
    async fn defer_from_hg(
        &self,
        ctx: &CoreContext,
        bonsai_hg_mapping: &dyn BonsaiHgMapping,
        hg_cs_id: &HgChangesetId,
    ) -> Result<Option<ChangesetId>, Error>;
}

// Overall trait with support for route tracking and handling
// partially derived types (it can see the node_data)
#[auto_impl(Arc)]
pub trait WalkVisitor<VOut, Route>: VisitOne {
    // Called before the step is attempted, returns None if step not needed
    fn start_step(
        &self,
        ctx: CoreContext,
        route: Option<&Route>,
        step: &OutgoingEdge,
    ) -> Option<CoreContext>;

    // This can mutate the internal state.  Takes ownership and returns data, plus next step
    fn visit(
        &self,
        ctx: &CoreContext,
        resolved: OutgoingEdge,
        node_data: Option<NodeData>,
        route: Option<Route>,
        outgoing: Vec<OutgoingEdge>,
    ) -> (VOut, Route, Vec<OutgoingEdge>);

    // For use when an edge should be visited in a later chunk
    fn defer_visit(
        &self,
        bcs_id: &ChangesetId,
        walk_item: &OutgoingEdge,
        route: Option<Route>,
    ) -> Result<(VOut, Route), Error>;
}

// Visitor methods that are only needed during tailing
pub trait TailingWalkVisitor {
    fn start_chunk(
        &mut self,
        chunk_members: &HashSet<ChangesetId>,
        mapping_prepop: Vec<BonsaiHgMappingEntry>,
    ) -> Result<HashSet<OutgoingEdge>, Error>;

    // WalkVisitor needs to be Arc for clone/move into spawn in walk.rs so we can't use &mut self to restrict this.
    // Should only called from tail.rs between chunks when nothing else is accessing the WalkVisitor.
    fn clear_state(
        &mut self,
        node_types: &HashSet<NodeType>,
        interned_types: &HashSet<InternedType>,
    );

    fn end_chunks(&mut self, logger: &Logger, contiguous_bounds: bool) -> Result<(), Error>;

    fn num_deferred(&self) -> usize;
}

// Data found for this node, plus next steps
enum StepOutput {
    Deferred(ChangesetId),
    Done(NodeData, Vec<OutgoingEdge>),
}

#[derive(Debug, Error)]
enum StepError {
    #[error("{0} is missing")]
    Missing(String),
    #[error("Hash validation failure: {0}")]
    HashValidationFailure(Error),
    #[error(transparent)]
    Other(#[from] Error),
}

impl From<LoadableError> for StepError {
    fn from(error: LoadableError) -> Self {
        match error {
            LoadableError::Missing(s) => StepError::Missing(s),
            LoadableError::Error(err) => StepError::Other(err),
        }
    }
}

async fn bookmark_step<V: VisitOne>(
    ctx: CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    b: BookmarkName,
    published_bookmarks: Arc<HashMap<BookmarkName, ChangesetId>>,
) -> Result<StepOutput, StepError> {
    let bcs_opt = match published_bookmarks.get(&b) {
        Some(csid) => Some(csid.clone()),
        // Just in case we have non-public bookmarks
        None => repo.bookmarks().get(ctx, &b).await?,
    };
    match bcs_opt {
        Some(bcs_id) => {
            let mut edges = vec![];
            checker.add_edge(&mut edges, EdgeType::BookmarkToChangeset, || {
                Node::Changeset(ChangesetKey {
                    inner: bcs_id,
                    filenode_known_derived: false, /* from bookmark we don't know if hg fully derived */
                })
            });
            checker.add_edge(&mut edges, EdgeType::BookmarkToBonsaiHgMapping, || {
                Node::BonsaiHgMapping(ChangesetKey {
                    inner: bcs_id,
                    filenode_known_derived: false, /* from bookmark we don't know if hg fully derived */
                })
            });
            Ok(StepOutput::Done(
                checker.step_data(NodeType::Bookmark, || NodeData::Bookmark(bcs_id)),
                edges,
            ))
        }
        None => Err(StepError::Missing(format!("Unknown Bookmark {}", b))),
    }
}

async fn published_bookmarks_step<V: VisitOne>(
    published_bookmarks: Arc<HashMap<BookmarkName, ChangesetId>>,
    checker: &Checker<V>,
) -> Result<StepOutput, StepError> {
    let mut edges = vec![];
    for (_, bcs_id) in published_bookmarks.iter() {
        checker.add_edge(&mut edges, EdgeType::PublishedBookmarksToChangeset, || {
            Node::Changeset(ChangesetKey {
                inner: bcs_id.clone(),
                filenode_known_derived: false, /* from bookmark we don't know if hg fully derived */
            })
        });
        checker.add_edge(
            &mut edges,
            EdgeType::PublishedBookmarksToBonsaiHgMapping,
            || {
                Node::BonsaiHgMapping(ChangesetKey {
                    inner: bcs_id.clone(),
                    filenode_known_derived: false, /* from bookmark we don't know if hg fully derived */
                })
            },
        );
    }
    Ok(StepOutput::Done(
        checker.step_data(NodeType::PublishedBookmarks, || {
            NodeData::PublishedBookmarks
        }),
        edges,
    ))
}

async fn bonsai_phase_step<V: VisitOne>(
    ctx: &CoreContext,
    checker: &Checker<V>,
    bcs_id: &ChangesetId,
) -> Result<StepOutput, StepError> {
    let maybe_phase = if checker.is_public(ctx, bcs_id).await? {
        Some(Phase::Public)
    } else {
        None
    };
    Ok(StepOutput::Done(
        checker.step_data(NodeType::PhaseMapping, || {
            NodeData::PhaseMapping(maybe_phase)
        }),
        vec![],
    ))
}

async fn blame_step<V: VisitOne>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    blame_id: BlameId,
) -> Result<StepOutput, StepError> {
    let blame = blame_id.load(ctx, repo.blobstore()).await?;
    let mut edges = vec![];

    if let BlameMaybeRejected::Blame(blame) = blame {
        for r in blame.ranges() {
            checker.add_edge(&mut edges, EdgeType::BlameToChangeset, || {
                Node::Changeset(ChangesetKey {
                    inner: r.csid,
                    filenode_known_derived: false, /* from blame we don't know if hg is fully derived */
                })
            });
        }
        Ok(StepOutput::Done(
            checker.step_data(NodeType::Blame, || NodeData::Blame(Some(blame))),
            edges,
        ))
    } else {
        Ok(StepOutput::Done(
            checker.step_data(NodeType::Blame, || NodeData::Blame(None)),
            edges,
        ))
    }
}

async fn fastlog_batch_step<V: VisitOne>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    id: &FastlogBatchId,
    path: Option<&WrappedPath>,
) -> Result<StepOutput, StepError> {
    let log = id.load(ctx, repo.blobstore()).await?;
    let mut edges = vec![];
    for (cs_id, _offsets) in log.latest() {
        checker.add_edge(&mut edges, EdgeType::FastlogBatchToChangeset, || {
            Node::Changeset(ChangesetKey {
                inner: *cs_id,
                filenode_known_derived: false, /* from log we don't know if hg is fully derived */
            })
        });
    }
    for id in log.previous_batches() {
        checker.add_edge_with_path(
            &mut edges,
            EdgeType::FastlogBatchToPreviousBatch,
            || Node::FastlogBatch(*id),
            || path.cloned(),
        );
    }
    Ok(StepOutput::Done(
        checker.step_data(NodeType::FastlogBatch, || NodeData::FastlogBatch(Some(log))),
        edges,
    ))
}

async fn fastlog_dir_step<V: VisitOne>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    id: &FastlogKey<ManifestUnodeId>,
    path: Option<&WrappedPath>,
) -> Result<StepOutput, StepError> {
    let log =
        fetch_fastlog_batch_by_unode_id(ctx, repo.blobstore(), &UnodeManifestEntry::Tree(id.inner))
            .await?;
    let mut edges = vec![];
    match &log {
        Some(log) => {
            for (cs_id, _offsets) in log.latest() {
                checker.add_edge(&mut edges, EdgeType::FastlogDirToChangeset, || {
                    Node::Changeset(ChangesetKey {
                        inner: *cs_id,
                        filenode_known_derived: false, /* from log we don't know if hg is fully derived */
                    })
                });
            }
            for id in log.previous_batches() {
                checker.add_edge_with_path(
                    &mut edges,
                    EdgeType::FastlogDirToPreviousBatch,
                    || Node::FastlogBatch(*id),
                    || path.cloned(),
                );
            }
        }
        None => {
            return Err(StepError::Missing(format!(
                "fastlog dir {} not found",
                id.inner
            )));
        }
    }

    Ok(StepOutput::Done(
        checker.step_data(NodeType::FastlogDir, || NodeData::FastlogDir(log)),
        edges,
    ))
}

async fn fastlog_file_step<V: VisitOne>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    id: &FastlogKey<FileUnodeId>,
    path: Option<&WrappedPath>,
) -> Result<StepOutput, StepError> {
    let log =
        fetch_fastlog_batch_by_unode_id(ctx, repo.blobstore(), &UnodeManifestEntry::Leaf(id.inner))
            .await?;
    let mut edges = vec![];
    match &log {
        Some(log) => {
            for (cs_id, _offsets) in log.latest() {
                checker.add_edge(&mut edges, EdgeType::FastlogFileToChangeset, || {
                    Node::Changeset(ChangesetKey {
                        inner: *cs_id,
                        filenode_known_derived: false, /* from log we don't know if hg is fully derived */
                    })
                });
            }
            for id in log.previous_batches() {
                checker.add_edge_with_path(
                    &mut edges,
                    EdgeType::FastlogFileToPreviousBatch,
                    || Node::FastlogBatch(*id),
                    || path.cloned(),
                );
            }
        }
        None => {
            return Err(StepError::Missing(format!(
                "fastlog file {} not found",
                id.inner
            )));
        }
    }

    Ok(StepOutput::Done(
        checker.step_data(NodeType::FastlogFile, || NodeData::FastlogFile(log)),
        edges,
    ))
}

async fn bonsai_changeset_info_mapping_step<V: VisitOne>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    bcs_id: ChangesetId,
    enable_derive: bool,
) -> Result<StepOutput, StepError> {
    if is_derived::<ChangesetInfo>(ctx, repo, bcs_id, enable_derive).await? {
        let mut edges = vec![];
        checker.add_edge(
            &mut edges,
            EdgeType::ChangesetInfoMappingToChangesetInfo,
            || Node::ChangesetInfo(bcs_id),
        );
        Ok(StepOutput::Done(
            checker.step_data(NodeType::ChangesetInfoMapping, || {
                NodeData::ChangesetInfoMapping(Some(bcs_id))
            }),
            edges,
        ))
    } else {
        Ok(StepOutput::Done(
            checker.step_data(NodeType::ChangesetInfoMapping, || {
                NodeData::ChangesetInfoMapping(None)
            }),
            vec![],
        ))
    }
}

async fn changeset_info_step<V: VisitOne>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    bcs_id: ChangesetId,
    enable_derive: bool,
) -> Result<StepOutput, StepError> {
    let info = maybe_derived::<ChangesetInfo>(ctx, repo, bcs_id, enable_derive).await?;

    if let Some(info) = info {
        let mut edges = vec![];
        for parent_id in info.parents() {
            checker.add_edge(
                &mut edges,
                EdgeType::ChangesetInfoToChangesetInfoParent,
                || Node::ChangesetInfo(parent_id),
            );
        }
        Ok(StepOutput::Done(
            checker.step_data(NodeType::ChangesetInfo, || {
                NodeData::ChangesetInfo(Some(info))
            }),
            edges,
        ))
    } else {
        Ok(StepOutput::Done(
            checker.step_data(NodeType::ChangesetInfo, || NodeData::ChangesetInfo(None)),
            vec![],
        ))
    }
}

async fn bonsai_changeset_step<V: VisitOne>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    key: &ChangesetKey<ChangesetId>,
) -> Result<StepOutput, StepError> {
    let bcs_id = &key.inner;

    // Get the data, and add direct file data for this bonsai changeset
    let bcs = bcs_id.load(ctx, repo.blobstore()).await?;

    // Build edges, from mostly queue expansion to least
    let mut edges = vec![];

    // Expands to parents
    checker.add_edge(
        &mut edges,
        EdgeType::ChangesetToChangesetInfoMapping,
        || Node::ChangesetInfoMapping(*bcs_id),
    );

    // Parents expand 1:[0|1|2] and then the same as all below
    for parent_id in bcs.parents() {
        checker.add_edge(&mut edges, EdgeType::ChangesetToBonsaiParent, || {
            Node::Changeset(ChangesetKey {
                inner: parent_id,
                filenode_known_derived: key.filenode_known_derived, /* if this has hg derived, so does parent */
            })
        });
    }
    // Unode mapping is 1:1 but from their expands considerably
    checker.add_edge(&mut edges, EdgeType::ChangesetToUnodeMapping, || {
        Node::UnodeMapping(*bcs_id)
    });
    // Fs node mapping is 1:1 but from their expands considerably
    checker.add_edge(&mut edges, EdgeType::ChangesetToFsnodeMapping, || {
        Node::FsnodeMapping(*bcs_id)
    });
    // Skeleton manifest mapping is 1:1 but from their expands less than unodes
    checker.add_edge(
        &mut edges,
        EdgeType::ChangesetToSkeletonManifestMapping,
        || Node::SkeletonManifestMapping(*bcs_id),
    );
    checker.add_edge(
        &mut edges,
        EdgeType::ChangesetToBasenameSuffixSkeletonManifestMapping,
        || Node::BasenameSuffixSkeletonManifestMapping(*bcs_id),
    );
    checker.add_edge(
        &mut edges,
        EdgeType::ChangesetToDeletedManifestV2Mapping,
        || Node::DeletedManifestV2Mapping(*bcs_id),
    );
    // Allow Hg based lookup which is 1:[1|0], may expand a lot from that
    checker.add_edge(&mut edges, EdgeType::ChangesetToBonsaiHgMapping, || {
        Node::BonsaiHgMapping(ChangesetKey {
            inner: *bcs_id,
            filenode_known_derived: key.filenode_known_derived,
        })
    });
    // File content expands just to meta+aliases 1:~5, with no further steps
    for (mpath, fc) in bcs.simplified_file_changes() {
        match fc {
            Some(tc) => {
                checker.add_edge_with_path(
                    &mut edges,
                    EdgeType::ChangesetToFileContent,
                    || Node::FileContent(tc.content_id()),
                    || Some(WrappedPath::from(Some(mpath.clone()))),
                );
            }
            None => {}
        }
    }
    // Phase mapping is 1:[0|1]
    checker.add_edge(&mut edges, EdgeType::ChangesetToPhaseMapping, || {
        Node::PhaseMapping(*bcs_id)
    });

    Ok(StepOutput::Done(
        checker.step_data(NodeType::Changeset, || NodeData::Changeset(bcs)),
        edges,
    ))
}

async fn file_content_step<V: VisitOne>(
    ctx: CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    id: ContentId,
) -> Result<StepOutput, StepError> {
    let maybe_s = filestore::fetch(repo.get_blobstore(), ctx, &id.into()).await?;
    let s = match maybe_s {
        Some(s) => s.map_ok(FileBytes),
        None => {
            return Err(StepError::Missing(format!("missing content for {}", id)));
        }
    };

    // We don't force file loading here, content may not be needed
    Ok(StepOutput::Done(
        checker.step_data(NodeType::FileContent, || {
            NodeData::FileContent(FileContentData::ContentStream(Box::pin(s)))
        }),
        vec![],
    ))
}

async fn file_content_metadata_step<V: VisitOne>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    id: ContentId,
    enable_derive: bool,
) -> Result<StepOutput, StepError> {
    let metadata_opt = if enable_derive {
        filestore::get_metadata(repo.blobstore(), ctx, &id.into())
            .await?
            .map(Some)
    } else {
        filestore::get_metadata_readonly(repo.blobstore(), ctx, &id.into()).await?
    };

    match metadata_opt {
        Some(Some(metadata)) => {
            let mut edges = vec![];
            checker.add_edge(&mut edges, EdgeType::FileContentMetadataToSha1Alias, || {
                Node::AliasContentMapping(AliasKey(Alias::Sha1(metadata.sha1)))
            });
            checker.add_edge(
                &mut edges,
                EdgeType::FileContentMetadataToSha256Alias,
                || Node::AliasContentMapping(AliasKey(Alias::Sha256(metadata.sha256))),
            );
            checker.add_edge(
                &mut edges,
                EdgeType::FileContentMetadataToGitSha1Alias,
                || Node::AliasContentMapping(AliasKey(Alias::GitSha1(metadata.git_sha1.sha1()))),
            );
            Ok(StepOutput::Done(
                checker.step_data(NodeType::FileContentMetadata, || {
                    NodeData::FileContentMetadata(Some(metadata))
                }),
                edges,
            ))
        }
        Some(None) | None => Ok(StepOutput::Done(
            checker.step_data(NodeType::FileContentMetadata, || {
                NodeData::FileContentMetadata(None)
            }),
            vec![],
        )),
    }
}

async fn evolve_filenode_flag<'a, V: 'a + VisitOne>(
    ctx: &'a CoreContext,
    repo: &'a BlobRepo,
    checker: &'a Checker<V>,
    key: ChangesetKey<ChangesetId>,
    enable_derive: bool,
) -> Result<bool, Error> {
    let mut filenode_known_derived = key.filenode_known_derived;

    if checker.with_filenodes && !filenode_known_derived {
        let bcs_id = key.inner;
        let derived_filenode = if enable_derive {
            if checker.is_public(ctx, &bcs_id).await? {
                let _ = FilenodesOnlyPublic::derive(ctx, repo, bcs_id)
                    .await
                    .map_err(Error::from)?;
                Some(true)
            } else {
                None
            }
        } else {
            None
        };

        // We only want to walk to Hg step if filenode is present
        filenode_known_derived = match derived_filenode {
            Some(v) => v,
            None => FilenodesOnlyPublic::is_derived(ctx, repo, &bcs_id)
                .await
                .map_err(Error::from)?,
        };
    }

    Ok(filenode_known_derived)
}

async fn bonsai_to_hg_key<'a, V: 'a + VisitOne>(
    ctx: &'a CoreContext,
    repo: &'a BlobRepo,
    checker: &'a Checker<V>,
    key: ChangesetKey<ChangesetId>,
    enable_derive: bool,
) -> Result<Option<ChangesetKey<HgChangesetId>>, Error> {
    let filenode_known_derived =
        evolve_filenode_flag(ctx, repo, checker, key.clone(), enable_derive).await?;

    if filenode_known_derived || !checker.with_filenodes {
        let bcs_id = key.inner;
        let from_state = checker.get_hg_from_bonsai(&bcs_id);
        let derived = if from_state.is_some() {
            from_state
        } else {
            maybe_derived::<MappedHgChangesetId>(ctx, repo, bcs_id, enable_derive)
                .await?
                .map(|v| {
                    checker.record_hg_from_bonsai(&bcs_id, v.hg_changeset_id());
                    v.hg_changeset_id()
                })
        };
        Ok(derived.map(|inner| ChangesetKey {
            inner,
            filenode_known_derived,
        }))
    } else {
        Ok(None)
    }
}

async fn bonsai_to_hg_mapping_step<'a, V: 'a + VisitOne>(
    ctx: &'a CoreContext,
    repo: &'a BlobRepo,
    checker: &'a Checker<V>,
    key: ChangesetKey<ChangesetId>,
    enable_derive: bool,
) -> Result<StepOutput, StepError> {
    let hg_key = bonsai_to_hg_key(ctx, repo, checker, key, enable_derive).await?;
    let mut edges = vec![];
    let hg_cs_id = hg_key.map(|hg_key| {
        // This seems like a nonsense edge, but its a way to establish HgChangesetId on the way to Bonsai Changeset
        // which is useful in LFS validation.  The edge is disabled by default.
        checker.add_edge(
            &mut edges,
            EdgeType::BonsaiHgMappingToHgBonsaiMapping,
            || Node::HgBonsaiMapping(hg_key.clone()),
        );
        checker.add_edge(
            &mut edges,
            // use HgChangesetViaBonsai rather than HgChangeset so that same route is taken to each changeset
            EdgeType::BonsaiHgMappingToHgChangesetViaBonsai,
            || Node::HgChangesetViaBonsai(hg_key.clone()),
        );
        hg_key.inner
    });

    Ok(StepOutput::Done(
        checker.step_data(NodeType::BonsaiHgMapping, || {
            NodeData::BonsaiHgMapping(hg_cs_id)
        }),
        edges,
    ))
}

async fn hg_to_bonsai_mapping_step<V: VisitOne>(
    ctx: &CoreContext,
    checker: &Checker<V>,
    key: ChangesetKey<HgChangesetId>,
) -> Result<StepOutput, StepError> {
    let bcs_id = checker.get_bonsai_from_hg(ctx, &key.inner).await?;

    let mut edges = vec![];
    checker.add_edge(&mut edges, EdgeType::HgBonsaiMappingToChangeset, || {
        Node::Changeset(ChangesetKey {
            inner: bcs_id,
            filenode_known_derived: key.filenode_known_derived,
        })
    });
    Ok(StepOutput::Done(
        checker.step_data(NodeType::HgBonsaiMapping, || {
            NodeData::HgBonsaiMapping(Some(bcs_id))
        }),
        edges,
    ))
}

async fn hg_changeset_via_bonsai_step<'a, V: VisitOne>(
    ctx: &'a CoreContext,
    repo: &'a BlobRepo,
    checker: &'a Checker<V>,
    input_key: ChangesetKey<HgChangesetId>,
    enable_derive: bool,
) -> Result<StepOutput, StepError> {
    let bcs_id = checker.get_bonsai_from_hg(ctx, &input_key.inner).await?;

    if !checker.in_chunk(&bcs_id) {
        return Ok(StepOutput::Deferred(bcs_id));
    }

    let bonsai_key = ChangesetKey {
        inner: bcs_id,
        filenode_known_derived: input_key.filenode_known_derived,
    };

    // Make sure we set the filenode flag for the step to HgChangeset
    let hg_key = ChangesetKey {
        inner: input_key.inner,
        filenode_known_derived: evolve_filenode_flag(ctx, repo, checker, bonsai_key, enable_derive)
            .await?,
    };

    let mut edges = vec![];
    checker.add_edge(
        &mut edges,
        EdgeType::HgChangesetViaBonsaiToHgChangeset,
        || Node::HgChangeset(hg_key),
    );
    Ok(StepOutput::Done(
        checker.step_data(NodeType::HgChangesetViaBonsai, || {
            NodeData::HgChangesetViaBonsai(input_key.inner)
        }),
        edges,
    ))
}

async fn hg_changeset_step<V: VisitOne>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    key: ChangesetKey<HgChangesetId>,
) -> Result<StepOutput, StepError> {
    let hgchangeset = key.inner.load(ctx, repo.blobstore()).await?;
    let mut edges = vec![];
    // 1:1 but will then expand a lot, usually
    checker.add_edge(&mut edges, EdgeType::HgChangesetToHgManifest, || {
        Node::HgManifest(PathKey::new(hgchangeset.manifestid(), WrappedPath::Root))
    });

    if key.filenode_known_derived {
        checker.add_edge(
            &mut edges,
            EdgeType::HgChangesetToHgManifestFileNode,
            || {
                Node::HgManifestFileNode(PathKey::new(
                    HgFileNodeId::new(hgchangeset.manifestid().into_nodehash()),
                    WrappedPath::Root,
                ))
            },
        );
    }

    // Mostly 1:1, can be 1:2, with further expansion
    for p in hgchangeset.parents().into_iter() {
        checker.add_edge(&mut edges, EdgeType::HgChangesetToHgParent, || {
            Node::HgChangesetViaBonsai(ChangesetKey {
                inner: HgChangesetId::new(p),
                filenode_known_derived: key.filenode_known_derived,
            })
        });
    }
    Ok(StepOutput::Done(
        checker.step_data(NodeType::HgChangeset, || NodeData::HgChangeset(hgchangeset)),
        edges,
    ))
}

async fn hg_file_envelope_step<V: VisitOne>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    hg_file_node_id: HgFileNodeId,
    path: Option<&WrappedPath>,
) -> Result<StepOutput, StepError> {
    let envelope = hg_file_node_id.load(ctx, repo.blobstore()).await?;
    let mut edges = vec![];
    checker.add_edge_with_path(
        &mut edges,
        EdgeType::HgFileEnvelopeToFileContent,
        || Node::FileContent(envelope.content_id()),
        || path.cloned(),
    );
    Ok(StepOutput::Done(
        checker.step_data(NodeType::HgFileEnvelope, || {
            NodeData::HgFileEnvelope(envelope)
        }),
        edges,
    ))
}

async fn file_node_step_impl<V: VisitOne, F, D>(
    ctx: CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    repo_path: RepoPath,
    path: WrappedPath,
    hg_file_node_id: HgFileNodeId,
    linknode_edge: EdgeType,
    linknode_mapping_edge: EdgeType,
    parent_edge: EdgeType,
    copyfrom_edge: EdgeType,
    build_file_node: F,
    build_data: D,
) -> Result<StepOutput, StepError>
where
    F: Fn(PathKey<HgFileNodeId>) -> Node,
    D: Fn(Option<FilenodeInfo>) -> NodeData,
{
    let file_node_info = repo
        .get_filenode_opt(ctx.clone(), &repo_path, hg_file_node_id)
        .await?
        .do_not_handle_disabled_filenodes()?;
    let mut edges = vec![];
    if let Some(file_node_info) = file_node_info.as_ref() {
        if let Some(bcs_id) = checker
            .defer_from_hg(&ctx, &file_node_info.linknode)
            .await?
        {
            return Ok(StepOutput::Deferred(bcs_id));
        }

        // Validate hg link node
        checker.add_edge(&mut edges, linknode_edge, || {
            Node::HgChangesetViaBonsai(ChangesetKey {
                inner: file_node_info.linknode,
                filenode_known_derived: true,
            })
        });

        // Following linknode bonsai increases parallelism of walk.
        // Linknodes will point to many commits we can then walk
        // in parallel
        checker.add_edge(&mut edges, linknode_mapping_edge, || {
            Node::HgBonsaiMapping(ChangesetKey {
                inner: file_node_info.linknode,
                filenode_known_derived: true,
            })
        });

        // Parents
        for parent in &[file_node_info.p1, file_node_info.p2] {
            if let Some(parent) = parent {
                checker.add_edge(&mut edges, parent_edge, || {
                    build_file_node(PathKey::new(*parent, path.clone()))
                })
            }
        }

        // Copyfrom is like another parent
        for (repo_path, file_node_id) in &file_node_info.copyfrom {
            checker.add_edge(&mut edges, copyfrom_edge, || {
                build_file_node(PathKey::new(
                    *file_node_id,
                    WrappedPath::from(repo_path.clone().into_mpath()),
                ))
            })
        }
    }

    Ok(StepOutput::Done(
        checker.step_data(parent_edge.outgoing_type(), || build_data(file_node_info)),
        edges,
    ))
}

async fn hg_file_node_step<V: VisitOne>(
    ctx: CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    path: WrappedPath,
    hg_file_node_id: HgFileNodeId,
) -> Result<StepOutput, StepError> {
    let repo_path = match &path {
        WrappedPath::Root => RepoPath::RootPath,
        WrappedPath::NonRoot(path) => RepoPath::FilePath(path.mpath().clone()),
    };
    file_node_step_impl(
        ctx,
        repo,
        checker,
        repo_path,
        path,
        hg_file_node_id,
        EdgeType::HgFileNodeToLinkedHgChangeset,
        EdgeType::HgFileNodeToLinkedHgBonsaiMapping,
        EdgeType::HgFileNodeToHgParentFileNode,
        EdgeType::HgFileNodeToHgCopyfromFileNode,
        Node::HgFileNode,
        NodeData::HgFileNode,
    )
    .await
}

async fn hg_manifest_file_node_step<V: VisitOne>(
    ctx: CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    path: WrappedPath,
    hg_file_node_id: HgFileNodeId,
) -> Result<StepOutput, StepError> {
    let repo_path = match &path {
        WrappedPath::Root => RepoPath::RootPath,
        WrappedPath::NonRoot(path) => RepoPath::DirectoryPath(path.mpath().clone()),
    };
    file_node_step_impl(
        ctx,
        repo,
        checker,
        repo_path,
        path,
        hg_file_node_id,
        EdgeType::HgManifestFileNodeToLinkedHgChangeset,
        EdgeType::HgManifestFileNodeToLinkedHgBonsaiMapping,
        EdgeType::HgManifestFileNodeToHgParentFileNode,
        EdgeType::HgManifestFileNodeToHgCopyfromFileNode,
        Node::HgManifestFileNode,
        NodeData::HgManifestFileNode,
    )
    .await
}

async fn hg_manifest_step<V: VisitOne>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    path: WrappedPath,
    hg_manifest_id: HgManifestId,
) -> Result<StepOutput, StepError> {
    let blobstore = repo.blobstore();
    let hgmanifest = hg_manifest_id.load(ctx, repo.blobstore()).await?;

    let mut edges = vec![];
    let mut filenode_edges = vec![];
    let mut envelope_edges = vec![];
    {
        let mut subentries = hgmanifest
            .list(ctx, blobstore)
            .await?
            .yield_every(MANIFEST_YIELD_EVERY_ENTRY_COUNT, |_| 1);
        while let Some((name, entry)) = subentries.try_next().await? {
            let full_path = WrappedPath::from(Some(MPath::join_opt_element(path.as_ref(), &name)));
            match entry {
                Entry::Leaf((_, hg_child_filenode_id)) => {
                    checker.add_edge_with_path(
                        &mut envelope_edges,
                        EdgeType::HgManifestToHgFileEnvelope,
                        || Node::HgFileEnvelope(hg_child_filenode_id),
                        || Some(full_path.clone()),
                    );
                    checker.add_edge(
                        &mut filenode_edges,
                        EdgeType::HgManifestToHgFileNode,
                        || Node::HgFileNode(PathKey::new(hg_child_filenode_id, full_path)),
                    );
                }
                Entry::Tree(hg_child_manifest_id) => {
                    checker.add_edge(
                        &mut filenode_edges,
                        EdgeType::HgManifestToHgManifestFileNode,
                        || {
                            Node::HgManifestFileNode(PathKey::new(
                                HgFileNodeId::new(hg_child_manifest_id.into_nodehash()),
                                full_path.clone(),
                            ))
                        },
                    );
                    checker.add_edge(&mut edges, EdgeType::HgManifestToChildHgManifest, || {
                        Node::HgManifest(PathKey::new(hg_child_manifest_id, full_path))
                    });
                }
            }
        }
    }

    // File nodes can expand a lot into history via linknodes
    edges.append(&mut filenode_edges);
    // Envelopes expand 1:1 to file content
    edges.append(&mut envelope_edges);

    Ok(StepOutput::Done(
        checker.step_data(NodeType::HgManifest, || NodeData::HgManifest(hgmanifest)),
        edges,
    ))
}

async fn alias_content_mapping_step<V: VisitOne>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    alias: Alias,
) -> Result<StepOutput, StepError> {
    let content_id = alias.load(ctx, repo.blobstore()).await?;
    let mut edges = vec![];
    checker.add_edge(
        &mut edges,
        EdgeType::AliasContentMappingToFileContent,
        || Node::FileContent(content_id),
    );
    Ok(StepOutput::Done(
        checker.step_data(NodeType::AliasContentMapping, || {
            NodeData::AliasContentMapping(content_id)
        }),
        edges,
    ))
}

// Only fetch if already derived unless enable_derive is set
async fn maybe_derived<Derived: BonsaiDerived>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    bcs_id: ChangesetId,
    enable_derive: bool,
) -> Result<Option<Derived>, Error> {
    if enable_derive {
        Ok(Some(Derived::derive(ctx, repo, bcs_id).await?))
    } else {
        Derived::fetch_derived(ctx, repo, &bcs_id).await
    }
}

// Variant of is_derived that will still trigger derivation if enable_derive is set
async fn is_derived<Derived: BonsaiDerived>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    bcs_id: ChangesetId,
    enable_derive: bool,
) -> Result<bool, Error> {
    if enable_derive {
        let _ = Derived::derive(ctx, repo, bcs_id).await?;
        Ok(true)
    } else {
        Ok(Derived::is_derived(ctx, repo, &bcs_id).await?)
    }
}

async fn bonsai_to_fsnode_mapping_step<V: VisitOne>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    bcs_id: ChangesetId,
    enable_derive: bool,
) -> Result<StepOutput, StepError> {
    let root_fsnode_id = maybe_derived::<RootFsnodeId>(ctx, repo, bcs_id, enable_derive).await?;

    if let Some(root_fsnode_id) = root_fsnode_id {
        let mut edges = vec![];
        checker.add_edge_with_path(
            &mut edges,
            EdgeType::FsnodeMappingToRootFsnode,
            || Node::Fsnode(*root_fsnode_id.fsnode_id()),
            || Some(WrappedPath::Root),
        );
        Ok(StepOutput::Done(
            checker.step_data(NodeType::FsnodeMapping, || {
                NodeData::FsnodeMapping(Some(*root_fsnode_id.fsnode_id()))
            }),
            edges,
        ))
    } else {
        Ok(StepOutput::Done(
            checker.step_data(NodeType::FsnodeMapping, || NodeData::FsnodeMapping(None)),
            vec![],
        ))
    }
}

async fn fsnode_step<V: VisitOne>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    fsnode_id: &FsnodeId,
    path: Option<&WrappedPath>,
) -> Result<StepOutput, StepError> {
    let fsnode = fsnode_id.load(ctx, &repo.get_blobstore()).await?;

    let mut content_edges = vec![];
    let mut dir_edges = vec![];
    {
        let mut children =
            stream::iter(fsnode.list()).yield_every(MANIFEST_YIELD_EVERY_ENTRY_COUNT, |_| 1);
        while let Some((child, fsnode_entry)) = children.next().await {
            // Fsnode do not have separate "file" entries, so we visit only directories
            match fsnode_entry {
                FsnodeEntry::Directory(dir) => {
                    let fsnode_id = dir.id();
                    checker.add_edge_with_path(
                        &mut dir_edges,
                        EdgeType::FsnodeToChildFsnode,
                        || Node::Fsnode(*fsnode_id),
                        || {
                            path.map(|p| {
                                WrappedPath::from(MPath::join_element_opt(p.as_ref(), Some(child)))
                            })
                        },
                    );
                }
                FsnodeEntry::File(file) => {
                    checker.add_edge_with_path(
                        &mut content_edges,
                        EdgeType::FsnodeToFileContent,
                        || Node::FileContent(*file.content_id()),
                        || {
                            path.map(|p| {
                                WrappedPath::from(MPath::join_element_opt(p.as_ref(), Some(child)))
                            })
                        },
                    );
                }
            }
        }
    }

    // Ordering to reduce queue depth
    dir_edges.append(&mut content_edges);

    Ok(StepOutput::Done(
        checker.step_data(NodeType::Fsnode, || NodeData::Fsnode(fsnode)),
        dir_edges,
    ))
}

async fn bonsai_to_unode_mapping_step<V: VisitOne>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    bcs_id: ChangesetId,
    enable_derive: bool,
) -> Result<StepOutput, StepError> {
    let mut root_unode_id =
        maybe_derived::<RootUnodeManifestId>(ctx, repo, bcs_id, enable_derive).await?;

    let mut walk_blame = checker.with_blame && root_unode_id.is_some();

    // If we need blame, need to make sure its derived also
    if walk_blame && !is_derived::<BlameRoot>(ctx, repo, bcs_id, enable_derive).await? {
        walk_blame = false;
        // Check if we should still walk the Unode even without blame
        if checker.is_public(ctx, &bcs_id).await? {
            // Do not proceed with step into unodes as public commit should have blame being derived
            // Private commits do not usually have blame, so they are ok to continue.
            root_unode_id = None;
        }
    }

    let mut walk_fastlog = checker.with_fastlog && root_unode_id.is_some();

    // If we need fastlog, need to make sure its derived also
    if walk_fastlog && !is_derived::<RootFastlog>(ctx, repo, bcs_id, enable_derive).await? {
        walk_fastlog = false;
        // Check if we should still walk the Unode even without fastlog
        if checker.is_public(ctx, &bcs_id).await? {
            // Do not proceed with step into unodes as public commit should have fastlog being derived
            // Private commits do not usually have fastlog, so they are ok to continue.
            root_unode_id = None;
        }
    }

    let mut flags = UnodeFlags::default();
    if walk_blame {
        flags |= UnodeFlags::BLAME;
    }
    if walk_fastlog {
        flags |= UnodeFlags::FASTLOG;
    }

    if let Some(root_unode_id) = root_unode_id {
        let mut edges = vec![];
        let manifest_id = *root_unode_id.manifest_unode_id();
        checker.add_edge_with_path(
            &mut edges,
            EdgeType::UnodeMappingToRootUnodeManifest,
            || {
                Node::UnodeManifest(UnodeKey {
                    inner: manifest_id,
                    flags,
                })
            },
            || Some(WrappedPath::Root),
        );
        Ok(StepOutput::Done(
            checker.step_data(NodeType::UnodeMapping, || {
                NodeData::UnodeMapping(Some(manifest_id))
            }),
            edges,
        ))
    } else {
        Ok(StepOutput::Done(
            checker.step_data(NodeType::UnodeMapping, || NodeData::UnodeMapping(None)),
            vec![],
        ))
    }
}

async fn unode_file_step<V: VisitOne>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    key: &UnodeKey<FileUnodeId>,
    path: Option<&WrappedPath>,
) -> Result<StepOutput, StepError> {
    let unode_file = key.inner.load(ctx, repo.blobstore()).await?;
    let linked_cs_id = *unode_file.linknode();
    if !checker.in_chunk(&linked_cs_id) {
        return Ok(StepOutput::Deferred(linked_cs_id));
    }

    let mut edges = vec![];

    // Check if we stepped from unode for non-public commit to unode for public, so can enable blame if required
    let walk_blame = checker.with_blame
        && (key.flags.contains(UnodeFlags::BLAME) || checker.is_public(ctx, &linked_cs_id).await?);

    let walk_fastlog = checker.with_fastlog
        && (key.flags.contains(UnodeFlags::FASTLOG)
            || checker.is_public(ctx, &linked_cs_id).await?);

    let mut flags = UnodeFlags::default();
    if walk_blame {
        flags |= UnodeFlags::BLAME;
        checker.add_edge(&mut edges, EdgeType::UnodeFileToBlame, || {
            Node::Blame(BlameId::from(key.inner))
        });
    }
    if walk_fastlog {
        flags |= UnodeFlags::FASTLOG;
        let path = &path;
        checker.add_edge_with_path(
            &mut edges,
            EdgeType::UnodeFileToFastlogFile,
            || Node::FastlogFile(FastlogKey::new(key.inner)),
            || path.cloned(),
        );
    }

    checker.add_edge(&mut edges, EdgeType::UnodeFileToLinkedChangeset, || {
        Node::Changeset(ChangesetKey {
            inner: linked_cs_id,
            filenode_known_derived: false, /* unode does not imply hg is fully derived */
        })
    });

    for p in unode_file.parents() {
        checker.add_edge_with_path(
            &mut edges,
            EdgeType::UnodeFileToUnodeFileParent,
            || Node::UnodeFile(UnodeKey { inner: *p, flags }),
            || path.cloned(),
        );
    }

    checker.add_edge_with_path(
        &mut edges,
        EdgeType::UnodeFileToFileContent,
        || Node::FileContent(*unode_file.content_id()),
        || path.cloned(),
    );

    Ok(StepOutput::Done(
        checker.step_data(NodeType::UnodeFile, || NodeData::UnodeFile(unode_file)),
        edges,
    ))
}

async fn unode_manifest_step<V: VisitOne>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    key: &UnodeKey<ManifestUnodeId>,
    path: Option<&WrappedPath>,
) -> Result<StepOutput, StepError> {
    let unode_manifest = key.inner.load(ctx, repo.blobstore()).await?;
    let linked_cs_id = *unode_manifest.linknode();
    if !checker.in_chunk(&linked_cs_id) {
        return Ok(StepOutput::Deferred(linked_cs_id));
    }

    let mut edges = vec![];

    checker.add_edge(&mut edges, EdgeType::UnodeManifestToLinkedChangeset, || {
        Node::Changeset(ChangesetKey {
            inner: linked_cs_id,
            filenode_known_derived: false, /* unode does not imply hg is fully derived */
        })
    });

    // Check if we stepped from unode for non-public commit to unode for public, so can enable blame if required
    let mut flags = UnodeFlags::default();
    if checker.with_blame
        && (key.flags.contains(UnodeFlags::BLAME) || checker.is_public(ctx, &linked_cs_id).await?)
    {
        flags |= UnodeFlags::BLAME;
    }

    // Check if we stepped from unode for non-public commit to unode for public, so can enable fastlog if required
    if checker.with_fastlog
        && (key.flags.contains(UnodeFlags::FASTLOG)
            || checker.is_public(ctx, &linked_cs_id).await?)
    {
        flags |= UnodeFlags::FASTLOG;
        let path = &path;
        checker.add_edge_with_path(
            &mut edges,
            EdgeType::UnodeManifestToFastlogDir,
            || Node::FastlogDir(FastlogKey::new(key.inner)),
            || path.cloned(),
        );
    }

    for p in unode_manifest.parents() {
        checker.add_edge_with_path(
            &mut edges,
            EdgeType::UnodeManifestToUnodeManifestParent,
            || Node::UnodeManifest(UnodeKey { inner: *p, flags }),
            || path.cloned(),
        );
    }

    let mut file_edges = vec![];
    for (child, subentry) in unode_manifest.subentries() {
        match subentry {
            UnodeEntry::Directory(id) => {
                checker.add_edge_with_path(
                    &mut edges,
                    EdgeType::UnodeManifestToUnodeManifestChild,
                    || Node::UnodeManifest(UnodeKey { inner: *id, flags }),
                    || {
                        path.map(|p| {
                            WrappedPath::from(MPath::join_element_opt(p.as_ref(), Some(child)))
                        })
                    },
                );
            }
            UnodeEntry::File(id) => {
                checker.add_edge_with_path(
                    &mut file_edges,
                    EdgeType::UnodeManifestToUnodeFileChild,
                    || Node::UnodeFile(UnodeKey { inner: *id, flags }),
                    || {
                        path.map(|p| {
                            WrappedPath::from(MPath::join_element_opt(p.as_ref(), Some(child)))
                        })
                    },
                );
            }
        }
    }

    // Ordering to reduce queue depth
    edges.append(&mut file_edges);

    Ok(StepOutput::Done(
        checker.step_data(NodeType::UnodeManifest, || {
            NodeData::UnodeManifest(unode_manifest)
        }),
        edges,
    ))
}

async fn deleted_manifest_v2_step<V: VisitOne>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    id: &DeletedManifestV2Id,
    path: Option<&WrappedPath>,
) -> Result<StepOutput, StepError> {
    let deleted_manifest_v2 = id.load(ctx, repo.blobstore()).await?;
    let linked_cs_id = deleted_manifest_v2.linknode().cloned();

    let mut edges = vec![];

    if let Some(linked_cs_id) = linked_cs_id {
        if !checker.in_chunk(&linked_cs_id) {
            return Ok(StepOutput::Deferred(linked_cs_id));
        }
        checker.add_edge(
            &mut edges,
            EdgeType::DeletedManifestV2ToLinkedChangeset,
            || {
                Node::Changeset(ChangesetKey {
                    inner: linked_cs_id,
                    filenode_known_derived: false, /* dfm does not imply hg is fully derived */
                })
            },
        );
    }

    let mut subentries = deleted_manifest_v2
        .clone()
        .into_subentries(ctx, repo.blobstore());

    while let Some((child_path, deleted_manifest_v2_id)) = subentries.try_next().await? {
        checker.add_edge_with_path(
            &mut edges,
            EdgeType::DeletedManifestV2ToDeletedManifestV2Child,
            || Node::DeletedManifestV2(deleted_manifest_v2_id),
            || {
                path.map(|p| {
                    WrappedPath::from(MPath::join_element_opt(p.as_ref(), Some(&child_path)))
                })
            },
        );
    }

    Ok(StepOutput::Done(
        checker.step_data(NodeType::DeletedManifestV2, || {
            NodeData::DeletedManifestV2(Some(deleted_manifest_v2))
        }),
        edges,
    ))
}

async fn deleted_manifest_v2_mapping_step<V: VisitOne>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    bcs_id: ChangesetId,
    enable_derive: bool,
) -> Result<StepOutput, StepError> {
    let root_manifest_v2_id =
        maybe_derived::<RootDeletedManifestV2Id>(ctx, repo, bcs_id, enable_derive).await?;

    if let Some(root_manifest_v2_id) = root_manifest_v2_id {
        let mut edges = vec![];
        checker.add_edge_with_path(
            &mut edges,
            EdgeType::DeletedManifestV2MappingToRootDeletedManifestV2,
            || Node::DeletedManifestV2(*root_manifest_v2_id.id()),
            || Some(WrappedPath::Root),
        );
        Ok(StepOutput::Done(
            checker.step_data(NodeType::DeletedManifestV2Mapping, || {
                NodeData::DeletedManifestV2Mapping(Some(*root_manifest_v2_id.id()))
            }),
            edges,
        ))
    } else {
        Ok(StepOutput::Done(
            checker.step_data(NodeType::DeletedManifestV2Mapping, || {
                NodeData::DeletedManifestV2Mapping(None)
            }),
            vec![],
        ))
    }
}

async fn skeleton_manifest_step<V: VisitOne>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    manifest_id: &SkeletonManifestId,
    path: Option<&WrappedPath>,
) -> Result<StepOutput, StepError> {
    let manifest = manifest_id.load(ctx, repo.blobstore()).await?;
    let mut edges = vec![];

    {
        let mut children =
            stream::iter(manifest.list()).yield_every(MANIFEST_YIELD_EVERY_ENTRY_COUNT, |_| 1);
        while let Some((child_path, entry)) = children.next().await {
            match entry {
                SkeletonManifestEntry::Directory(subdir) => {
                    checker.add_edge_with_path(
                        &mut edges,
                        EdgeType::SkeletonManifestToSkeletonManifestChild,
                        || Node::SkeletonManifest(*subdir.id()),
                        || {
                            path.map(|p| {
                                WrappedPath::from(MPath::join_element_opt(
                                    p.as_ref(),
                                    Some(child_path),
                                ))
                            })
                        },
                    );
                }
                SkeletonManifestEntry::File => {}
            }
        }
    }

    Ok(StepOutput::Done(
        checker.step_data(NodeType::SkeletonManifest, || {
            NodeData::SkeletonManifest(Some(manifest))
        }),
        edges,
    ))
}

async fn skeleton_manifest_mapping_step<V: VisitOne>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    bcs_id: ChangesetId,
    enable_derive: bool,
) -> Result<StepOutput, StepError> {
    let root_manifest_id =
        maybe_derived::<RootSkeletonManifestId>(ctx, repo, bcs_id, enable_derive).await?;

    if let Some(root_manifest_id) = root_manifest_id {
        let mut edges = vec![];

        checker.add_edge_with_path(
            &mut edges,
            EdgeType::SkeletonManifestMappingToRootSkeletonManifest,
            || Node::SkeletonManifest(*root_manifest_id.skeleton_manifest_id()),
            || Some(WrappedPath::Root),
        );
        Ok(StepOutput::Done(
            checker.step_data(NodeType::SkeletonManifestMapping, || {
                NodeData::SkeletonManifestMapping(Some(*root_manifest_id.skeleton_manifest_id()))
            }),
            edges,
        ))
    } else {
        Ok(StepOutput::Done(
            checker.step_data(NodeType::SkeletonManifestMapping, || {
                NodeData::SkeletonManifestMapping(None)
            }),
            vec![],
        ))
    }
}

async fn basename_suffix_skeleton_manifest_step<V: VisitOne>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    manifest_id: &BasenameSuffixSkeletonManifestId,
    path: Option<&WrappedPath>,
) -> Result<StepOutput, StepError> {
    let manifest = manifest_id.load(ctx, repo.blobstore()).await?;
    let mut edges = vec![];
    {
        let mut children = manifest
            .list(ctx, repo.blobstore())
            .await?
            .yield_every(MANIFEST_YIELD_EVERY_ENTRY_COUNT, |_| 1);

        while let Some((child_path, entry)) = children.try_next().await? {
            match entry {
                manifest::Entry::Tree(subdir) => {
                    checker.add_edge_with_path(
                    &mut edges,
                    EdgeType::BasenameSuffixSkeletonManifestToBasenameSuffixSkeletonManifestChild,
                    || Node::BasenameSuffixSkeletonManifest(subdir.id),
                    || {
                        path.map(|p| {
                            WrappedPath::from(MPath::join_element_opt(
                                p.as_ref(),
                                Some(&child_path),
                            ))
                        })
                    },
                );
                }
                manifest::Entry::Leaf(()) => {}
            }
        }
    }

    Ok(StepOutput::Done(
        checker.step_data(NodeType::BasenameSuffixSkeletonManifest, || {
            NodeData::BasenameSuffixSkeletonManifest(Some(manifest))
        }),
        edges,
    ))
}

async fn basename_suffix_skeleton_manifest_mapping_step<V: VisitOne>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    bcs_id: ChangesetId,
    enable_derive: bool,
) -> Result<StepOutput, StepError> {
    let root_manifest =
        maybe_derived::<RootBasenameSuffixSkeletonManifest>(ctx, repo, bcs_id, enable_derive)
            .await?;

    if let Some(root_manifest) = root_manifest {
        let mut edges = vec![];
        let id = root_manifest.into_inner_id();

        checker.add_edge_with_path(
            &mut edges,
            EdgeType::BasenameSuffixSkeletonManifestMappingToRootBasenameSuffixSkeletonManifest,
            || Node::BasenameSuffixSkeletonManifest(id),
            || Some(WrappedPath::Root),
        );
        Ok(StepOutput::Done(
            checker.step_data(NodeType::BasenameSuffixSkeletonManifestMapping, || {
                NodeData::BasenameSuffixSkeletonManifestMapping(Some(id))
            }),
            edges,
        ))
    } else {
        Ok(StepOutput::Done(
            checker.step_data(NodeType::BasenameSuffixSkeletonManifestMapping, || {
                NodeData::BasenameSuffixSkeletonManifestMapping(None)
            }),
            vec![],
        ))
    }
}

/// Expand nodes where check for a type is used as a check for other types.
/// e.g. to make sure metadata looked up/considered for files.
pub fn expand_checked_nodes(children: &mut Vec<OutgoingEdge>) {
    let mut extra = vec![];
    for n in children.iter() {
        match n {
            OutgoingEdge {
                label: _,
                target: Node::FileContent(fc_id),
                path,
            } => {
                extra.push(OutgoingEdge::new_with_path(
                    EdgeType::FileContentToFileContentMetadata,
                    Node::FileContentMetadata(*fc_id),
                    path.clone(),
                ));
            }
            _ => {}
        }
    }
    if !extra.is_empty() {
        children.append(&mut extra);
    }
}

struct Checker<V: VisitOne> {
    include_edge_types: HashSet<EdgeType>,
    hash_validation_node_types: HashSet<NodeType>,
    always_emit_edge_types: HashSet<EdgeType>,
    required_node_data_types: HashSet<NodeType>,
    keep_edge_paths: bool,
    visitor: V,
    phases_store: Arc<dyn Phases>,
    bonsai_hg_mapping: Arc<dyn BonsaiHgMapping>,
    with_blame: bool,
    with_fastlog: bool,
    with_filenodes: bool,
}

impl<V: VisitOne> Checker<V> {
    async fn is_public(&self, ctx: &CoreContext, bcs_id: &ChangesetId) -> Result<bool, Error> {
        self.visitor
            .is_public(ctx, self.phases_store.as_ref(), bcs_id)
            .await
    }

    fn in_chunk(&self, bcs_id: &ChangesetId) -> bool {
        self.visitor.in_chunk(bcs_id)
    }

    fn get_hg_from_bonsai(&self, bcs_id: &ChangesetId) -> Option<HgChangesetId> {
        self.visitor.get_hg_from_bonsai(bcs_id)
    }

    fn record_hg_from_bonsai(&self, bcs_id: &ChangesetId, hg_cs_id: HgChangesetId) {
        self.visitor.record_hg_from_bonsai(bcs_id, hg_cs_id)
    }

    async fn get_bonsai_from_hg(
        &self,
        ctx: &CoreContext,
        hg_cs_id: &HgChangesetId,
    ) -> Result<ChangesetId, Error> {
        self.visitor
            .get_bonsai_from_hg(ctx, self.bonsai_hg_mapping.as_ref(), hg_cs_id)
            .await
    }

    async fn defer_from_hg(
        &self,
        ctx: &CoreContext,
        hg_cs_id: &HgChangesetId,
    ) -> Result<Option<ChangesetId>, Error> {
        self.visitor
            .defer_from_hg(ctx, self.bonsai_hg_mapping.as_ref(), hg_cs_id)
            .await
    }

    // Convience method around make_edge
    fn add_edge<N>(&self, edges: &mut Vec<OutgoingEdge>, edge_type: EdgeType, node_fn: N)
    where
        N: FnOnce() -> Node,
    {
        if let Some(edge) = self.make_edge(edge_type, node_fn) {
            edges.push(edge)
        }
    }

    // Convience method around make_edge_with_path
    fn add_edge_with_path<N, P>(
        &self,
        edges: &mut Vec<OutgoingEdge>,
        edge_type: EdgeType,
        node_fn: N,
        path_fn: P,
    ) where
        N: FnOnce() -> Node,
        P: FnOnce() -> Option<WrappedPath>,
    {
        if let Some(edge) = self.make_edge_with_path(edge_type, node_fn, path_fn) {
            edges.push(edge)
        }
    }

    // Construct a new edge, only calling visitor to check if the edge_type is needed
    fn make_edge<N>(&self, edge_type: EdgeType, node_fn: N) -> Option<OutgoingEdge>
    where
        N: FnOnce() -> Node,
    {
        let always_emit = self.always_emit_edge_types.contains(&edge_type);
        if always_emit || self.include_edge_types.contains(&edge_type) {
            let outgoing = OutgoingEdge::new(edge_type, node_fn());
            if always_emit || self.visitor.needs_visit(&outgoing) {
                return Some(outgoing);
            }
        }
        None
    }

    // Construct a new edge, only calling visitor to check if the edge_type is needed
    fn make_edge_with_path<N, P>(
        &self,
        edge_type: EdgeType,
        node_fn: N,
        path_fn: P,
    ) -> Option<OutgoingEdge>
    where
        N: FnOnce() -> Node,
        P: FnOnce() -> Option<WrappedPath>,
    {
        let always_emit = self.always_emit_edge_types.contains(&edge_type);
        if always_emit || self.include_edge_types.contains(&edge_type) {
            let outgoing = if self.keep_edge_paths {
                OutgoingEdge::new_with_path(edge_type, node_fn(), path_fn())
            } else {
                OutgoingEdge::new(edge_type, node_fn())
            };
            if always_emit || self.visitor.needs_visit(&outgoing) {
                return Some(outgoing);
            }
        }
        None
    }

    // Only add the node data if requested
    fn step_data<D>(&self, t: NodeType, data_fn: D) -> NodeData
    where
        D: FnOnce() -> NodeData,
    {
        if self.required_node_data_types.contains(&t) {
            data_fn()
        } else {
            NodeData::NotRequired
        }
    }
}

// Parameters that vary per repo but can be setup in common conde
#[derive(Clone)]
pub struct RepoWalkParams {
    pub repo: BlobRepo,
    pub logger: Logger,
    pub scuba_builder: MononokeScubaSampleBuilder,
    pub scheduled_max: usize,
    pub sql_shard_info: SqlShardInfo,
    pub walk_roots: Vec<OutgoingEdge>,
    pub include_node_types: HashSet<NodeType>,
    pub include_edge_types: HashSet<EdgeType>,
    pub hash_validation_node_types: HashSet<NodeType>,
}

// Parameters that vary per repo but are set differently by scrub, validate etc.
#[derive(Clone, Default)]
pub struct RepoWalkTypeParams {
    pub always_emit_edge_types: HashSet<EdgeType>,
    pub required_node_data_types: HashSet<NodeType>,
    pub keep_edge_paths: bool,
}

/// Walk the graph from one or more starting points,  providing stream of data for later reduction
pub fn walk_exact<V, VOut, Route>(
    ctx: CoreContext,
    visitor: V,
    job_params: JobWalkParams,
    repo_params: RepoWalkParams,
    type_params: RepoWalkTypeParams,
) -> impl Stream<Item = Result<VOut, Error>>
where
    V: 'static + Clone + WalkVisitor<VOut, Route> + Send + Sync,
    VOut: 'static + Send,
    Route: 'static + Send + Clone + StepRoute,
{
    // Build lookups
    let published_bookmarks = repo_params
        .repo
        .bookmarks()
        .list(
            ctx.clone(),
            Freshness::MostRecent,
            &BookmarkPrefix::empty(),
            BookmarkKind::ALL_PUBLISHING,
            &BookmarkPagination::FromStart,
            std::u64::MAX,
        )
        .map_ok(|(book, csid)| (book.name, csid))
        .try_collect::<HashMap<_, _>>();

    // Roots were not stepped to from elsewhere, so their Option<Route> is None.
    let walk_roots: Vec<(Option<Route>, OutgoingEdge)> = repo_params
        .walk_roots
        .iter()
        .map(|e| (None, e.clone()))
        .collect();

    async move {
        let published_bookmarks = Arc::new(published_bookmarks.await?);
        let heads = published_bookmarks
            .iter()
            .map(|(_, csid)| *csid)
            .collect::<Vec<_>>();

        cloned!(
            repo_params.repo,
            repo_params.include_edge_types,
            repo_params.hash_validation_node_types,
            repo_params.include_node_types,
            repo_params.sql_shard_info,
        );

        let mut required_node_data_types = type_params.required_node_data_types;
        required_node_data_types.extend(hash_validation_node_types.clone());
        let checker = Arc::new(Checker {
            with_blame: repo_params.include_node_types.contains(&NodeType::Blame),
            with_fastlog: include_node_types
                .iter()
                .any(|n| n.derived_data_name() == Some(RootFastlog::NAME)),
            with_filenodes: include_edge_types.iter().any(|e| {
                e.outgoing_type() == NodeType::HgFileNode
                    || e.outgoing_type() == NodeType::HgManifestFileNode
            }),
            include_edge_types,
            hash_validation_node_types,
            always_emit_edge_types: type_params.always_emit_edge_types,
            keep_edge_paths: type_params.keep_edge_paths,
            visitor: visitor.clone(),
            required_node_data_types,
            phases_store: repo.phases().with_frozen_public_heads(heads),
            bonsai_hg_mapping: repo.bonsai_hg_mapping_arc().clone(),
        });

        Ok(limited_by_key_shardable(
            repo_params.scheduled_max,
            walk_roots,
            move |(via, walk_item): (Option<Route>, OutgoingEdge)| {
                cloned!(repo_params.sql_shard_info);
                let shard_key = walk_item.target.sql_shard(&sql_shard_info);
                let ctx =
                    if let Some(ctx) = visitor.start_step(ctx.clone(), via.as_ref(), &walk_item) {
                        ctx
                    } else {
                        info!(ctx.logger(), #log::SUPPRESS, "Suppressing edge {:?}", walk_item);
                        return future::ready((walk_item.target, shard_key, Ok(None))).boxed();
                    };

                cloned!(
                    job_params.error_as_data_node_types,
                    job_params.error_as_data_edge_types,
                    job_params.enable_derive,
                    published_bookmarks,
                    repo_params.repo,
                    repo_params.scuba_builder,
                    visitor,
                    checker,
                    walk_item.target,
                );

                // Each step returns the walk result, and next steps
                async move {
                    let next = walk_one(
                        ctx,
                        via,
                        walk_item,
                        repo,
                        enable_derive,
                        visitor,
                        error_as_data_node_types,
                        error_as_data_edge_types,
                        scuba_builder,
                        published_bookmarks,
                        checker,
                    );

                    let handle = tokio::task::spawn(next);
                    handle.await?
                }
                .map(move |v| (target, shard_key, v))
                .boxed()
            },
            move |(_route, edge)| {
                (
                    &edge.target,
                    sql_shard_info
                        .active_keys_per_shard
                        .as_ref()
                        .and_then(|per_shard| {
                            edge.target
                                .sql_shard(&sql_shard_info)
                                .map(|v| (v, *per_shard))
                        }),
                )
            },
        ))
    }
    .try_flatten_stream()
}

async fn walk_one<V, VOut, Route>(
    ctx: CoreContext,
    via: Option<Route>,
    walk_item: OutgoingEdge,
    repo: BlobRepo,
    enable_derive: bool,
    visitor: V,
    error_as_data_node_types: HashSet<NodeType>,
    error_as_data_edge_types: HashSet<EdgeType>,
    mut scuba: MononokeScubaSampleBuilder,
    published_bookmarks: Arc<HashMap<BookmarkName, ChangesetId>>,
    checker: Arc<Checker<V>>,
) -> Result<
    Option<(
        VOut,
        impl IntoIterator<Item = (Option<Route>, OutgoingEdge)>,
    )>,
    Error,
>
where
    V: 'static + Clone + WalkVisitor<VOut, Route> + Send,
    VOut: 'static + Send,
    Route: 'static + Send + Clone + StepRoute,
{
    let logger = ctx.logger().clone();

    if via.is_none() {
        // record stats for the walk_roots
        visitor.visit(&ctx, walk_item.clone(), None, None, vec![walk_item.clone()]);
    }

    let step_result = match walk_item.target.clone() {
        Node::Root(_) => Err(StepError::Other(format_err!(
            "Not expecting Roots to be generated"
        ))),
        // Bonsai
        Node::Bookmark(bookmark_name) => {
            bookmark_step(
                ctx.clone(),
                &repo,
                &checker,
                bookmark_name,
                published_bookmarks.clone(),
            )
            .await
        }
        Node::Changeset(key) => bonsai_changeset_step(&ctx, &repo, &checker, &key).await,
        Node::BonsaiHgMapping(bcs_id) => {
            bonsai_to_hg_mapping_step(&ctx, &repo, &checker, bcs_id, enable_derive).await
        }
        Node::PhaseMapping(bcs_id) => bonsai_phase_step(&ctx, &checker, &bcs_id).await,
        Node::PublishedBookmarks(_) => {
            published_bookmarks_step(published_bookmarks.clone(), &checker).await
        }
        // Hg
        Node::HgBonsaiMapping(key) => hg_to_bonsai_mapping_step(&ctx, &checker, key).await,
        Node::HgChangeset(hg_csid) => hg_changeset_step(&ctx, &repo, &checker, hg_csid).await,
        Node::HgChangesetViaBonsai(hg_csid) => {
            hg_changeset_via_bonsai_step(&ctx, &repo, &checker, hg_csid, enable_derive).await
        }
        Node::HgFileEnvelope(hg_file_node_id) => {
            hg_file_envelope_step(
                &ctx,
                &repo,
                &checker,
                hg_file_node_id,
                walk_item.path.as_ref(),
            )
            .await
        }
        Node::HgFileNode(PathKey { id, path }) => {
            hg_file_node_step(ctx.clone(), &repo, &checker, path, id).await
        }
        Node::HgManifestFileNode(PathKey { id, path }) => {
            hg_manifest_file_node_step(ctx.clone(), &repo, &checker, path, id).await
        }
        Node::HgManifest(PathKey { id, path }) => {
            hg_manifest_step(&ctx, &repo, &checker, path, id).await
        }
        // Content
        Node::FileContent(content_id) => {
            file_content_step(ctx.clone(), &repo, &checker, content_id).await
        }
        Node::FileContentMetadata(content_id) => {
            file_content_metadata_step(&ctx, &repo, &checker, content_id, enable_derive).await
        }
        Node::AliasContentMapping(AliasKey(alias)) => {
            alias_content_mapping_step(&ctx, &repo, &checker, alias).await
        }
        // Derived
        Node::Blame(blame_id) => blame_step(&ctx, &repo, &checker, blame_id).await,
        Node::ChangesetInfo(bcs_id) => {
            changeset_info_step(&ctx, &repo, &checker, bcs_id, enable_derive).await
        }
        Node::ChangesetInfoMapping(bcs_id) => {
            bonsai_changeset_info_mapping_step(&ctx, &repo, &checker, bcs_id, enable_derive).await
        }
        Node::DeletedManifestV2(id) => {
            deleted_manifest_v2_step(&ctx, &repo, &checker, &id, walk_item.path.as_ref()).await
        }
        Node::DeletedManifestV2Mapping(bcs_id) => {
            deleted_manifest_v2_mapping_step(&ctx, &repo, &checker, bcs_id, enable_derive).await
        }
        Node::FastlogBatch(id) => {
            fastlog_batch_step(&ctx, &repo, &checker, &id, walk_item.path.as_ref()).await
        }
        Node::FastlogDir(id) => {
            fastlog_dir_step(&ctx, &repo, &checker, &id, walk_item.path.as_ref()).await
        }
        Node::FastlogFile(id) => {
            fastlog_file_step(&ctx, &repo, &checker, &id, walk_item.path.as_ref()).await
        }
        Node::Fsnode(id) => fsnode_step(&ctx, &repo, &checker, &id, walk_item.path.as_ref()).await,
        Node::FsnodeMapping(bcs_id) => {
            bonsai_to_fsnode_mapping_step(&ctx, &repo, &checker, bcs_id, enable_derive).await
        }
        Node::SkeletonManifest(id) => {
            skeleton_manifest_step(&ctx, &repo, &checker, &id, walk_item.path.as_ref()).await
        }
        Node::SkeletonManifestMapping(bcs_id) => {
            skeleton_manifest_mapping_step(&ctx, &repo, &checker, bcs_id, enable_derive).await
        }
        Node::BasenameSuffixSkeletonManifest(id) => {
            basename_suffix_skeleton_manifest_step(
                &ctx,
                &repo,
                &checker,
                &id,
                walk_item.path.as_ref(),
            )
            .await
        }
        Node::BasenameSuffixSkeletonManifestMapping(bcs_id) => {
            basename_suffix_skeleton_manifest_mapping_step(
                &ctx,
                &repo,
                &checker,
                bcs_id,
                enable_derive,
            )
            .await
        }
        Node::UnodeFile(id) => {
            unode_file_step(&ctx, &repo, &checker, &id, walk_item.path.as_ref()).await
        }
        Node::UnodeManifest(id) => {
            unode_manifest_step(&ctx, &repo, &checker, &id, walk_item.path.as_ref()).await
        }
        Node::UnodeMapping(bcs_id) => {
            bonsai_to_unode_mapping_step(&ctx, &repo, &checker, bcs_id, enable_derive).await
        }
    };

    let edge_label = walk_item.label;
    let node_type = walk_item.target.get_type();

    // Run hash validation if needed
    let step_result = match step_result {
        Ok(StepOutput::Done(node_data, children)) => {
            if checker.hash_validation_node_types.contains(&node_type) {
                let f = walk_item
                    .target
                    .validate_hash(ctx.clone(), repo.clone(), &node_data);
                match f.await {
                    Ok(()) => Ok(StepOutput::Done(node_data, children)),
                    Err(err @ HashValidationError::HashMismatch { .. }) => {
                        Err(StepError::HashValidationFailure(format_err!("{:?}", err)))
                    }
                    Err(HashValidationError::Error(err)) => {
                        return Err(err);
                    }
                    Err(HashValidationError::NotSupported(err)) => {
                        return Err(format_err!("{}", err));
                    }
                }
            } else {
                Ok(StepOutput::Done(node_data, children))
            }
        }
        res => res,
    };

    let step_output = match step_result {
        Ok(s) => Ok(s),
        Err(e) => {
            let msg = format!(
                "Could not step to {:?}, due to {:?}, via {:?}",
                &walk_item, e, via
            );
            // Log to scuba regardless
            add_node_to_scuba(
                via.as_ref().and_then(|v| v.source_node()),
                via.as_ref().and_then(|v| v.via_node()),
                &walk_item.target,
                walk_item.path.as_ref(),
                &mut scuba,
            );

            let check_type = match e {
                StepError::Missing(_) => "missing",
                StepError::HashValidationFailure(_) => "hash_validation_failure",
                StepError::Other(_) => "step",
            };

            scuba
                .add(EDGE_TYPE, Into::<&'static str>::into(edge_label))
                .add(CHECK_TYPE, check_type)
                .add(CHECK_FAIL, 1)
                .add(ERROR_MSG, msg.clone())
                .log();
            // Optionally attempt to continue
            if error_as_data_node_types.contains(&walk_item.target.get_type()) {
                if error_as_data_edge_types.is_empty()
                    || error_as_data_edge_types.contains(&walk_item.label)
                {
                    warn!(logger, "{}", msg);
                    match e {
                        StepError::Missing(_s) => Ok(StepOutput::Done(
                            NodeData::MissingAsData(walk_item.target.clone()),
                            vec![],
                        )),
                        StepError::HashValidationFailure(_s) => Ok(StepOutput::Done(
                            NodeData::HashValidationFailureAsData(walk_item.target.clone()),
                            vec![],
                        )),
                        StepError::Other(_e) => Ok(StepOutput::Done(
                            NodeData::ErrorAsData(walk_item.target.clone()),
                            vec![],
                        )),
                    }
                } else {
                    Err(e)
                }
            } else {
                Err(e)
            }
        }
    }
    .with_context(|| {
        ErrorKind::NotTraversable(repo.name().clone(), walk_item.clone(), format!("{:?}", via))
    })?;

    let (vout, via, next) = match step_output {
        StepOutput::Deferred(bcs_id) => {
            let (vout, via) = visitor.defer_visit(&bcs_id, &walk_item, via)?;
            (vout, via, vec![])
        }
        StepOutput::Done(node_data, children) => {
            // make sure steps are valid.  would be nice if this could be static
            for c in &children {
                if c.label.outgoing_type() != c.target.get_type() {
                    return Err(format_err!(
                        "Bad step {:?} to {:?}",
                        c.label,
                        c.target.get_type()
                    ));
                } else if c.label.incoming_type().map_or(false, |t| t != node_type) {
                    return Err(format_err!("Bad step {:?} from {:?}", c.label, node_type,));
                }
            }

            // Allow WalkVisitor to record state and decline outgoing nodes if already visited
            visitor.visit(&ctx, walk_item, Some(node_data), via, children)
        }
    };
    let via = Some(via);
    let next = next.into_iter().map(move |e| (via.clone(), e));
    Ok(Some((vout, next)))
}
