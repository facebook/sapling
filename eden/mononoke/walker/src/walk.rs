/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::graph::{
    AliasKey, EdgeType, FastlogKey, FileContentData, Node, NodeData, NodeType, PathKey, UnodeFlags,
    UnodeKey, UnodeManifestEntry, WrappedPath,
};
use crate::validate::{add_node_to_scuba, CHECK_FAIL, CHECK_TYPE, EDGE_TYPE};

use anyhow::{format_err, Context, Error};
use async_trait::async_trait;
use auto_impl::auto_impl;
use blame::BlameRoot;
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use bookmarks::{BookmarkKind, BookmarkName, BookmarkPagination, BookmarkPrefix, Freshness};
use bounded_traversal::bounded_traversal_stream;
use changeset_info::ChangesetInfo;
use cloned::cloned;
use context::CoreContext;
use deleted_files_manifest::RootDeletedManifestId;
use derived_data::BonsaiDerived;
use derived_data_filenodes::FilenodesOnlyPublic;
use fastlog::{fetch_fastlog_batch_by_unode_id, RootFastlog};
use filestore::{self, Alias};
use fsnodes::RootFsnodeId;
use futures::{
    compat::Future01CompatExt,
    future::{self, FutureExt, TryFutureExt},
    stream::{BoxStream, StreamExt, TryStreamExt},
};
use futures_old::Future as Future01;
use itertools::{Either, Itertools};
use manifest::{Entry, Manifest};
use mercurial_derived_data::MappedHgChangesetId;
use mercurial_types::{FileBytes, HgChangesetId, HgFileNodeId, HgManifestId, RepoPath};
use mononoke_types::{
    blame::BlameMaybeRejected, fsnode::FsnodeEntry, skeleton_manifest::SkeletonManifestEntry,
    unode::UnodeEntry, BlameId, ChangesetId, ContentId, DeletedManifestId, FastlogBatchId,
    FileUnodeId, FsnodeId, MPath, ManifestUnodeId, SkeletonManifestId,
};
use phases::{HeadsFetcher, Phase, Phases};
use scuba_ext::MononokeScubaSampleBuilder;
use skeleton_manifest::RootSkeletonManifestId;
use slog::warn;
use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    iter::{IntoIterator, Iterator},
    sync::Arc,
};
use thiserror::Error;
use unodes::RootUnodeManifestId;

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
    #[error("Could not step to {0:?} via {1}")]
    NotTraversable(OutgoingEdge, String),
}

// Simpler visitor trait used inside each step to decide
// whether to emit an edge
#[async_trait]
#[auto_impl(Arc)]
pub trait VisitOne {
    fn needs_visit(&self, outgoing: &OutgoingEdge) -> bool;

    async fn is_public(
        &self,
        ctx: &CoreContext,
        phases_store: &dyn Phases,
        bcs_id: &ChangesetId,
    ) -> Result<bool, Error>;
}

// Overall trait with support for route tracking and handling
// partially derived types (it can see the node_data)
#[auto_impl(Arc)]
pub trait WalkVisitor<VOut, Route>: VisitOne {
    // Called before the step is attempted
    fn start_step(
        &self,
        ctx: CoreContext,
        route: Option<&Route>,
        step: &OutgoingEdge,
    ) -> CoreContext;

    // This can mutate the internal state.  Takes ownership and returns data, plus next step
    fn visit(
        &self,
        ctx: &CoreContext,
        resolved: OutgoingEdge,
        node_data: Option<NodeData>,
        route: Option<Route>,
        outgoing: Vec<OutgoingEdge>,
    ) -> (VOut, Route, Vec<OutgoingEdge>);
}

// Data found for this node, plus next steps
struct StepOutput(NodeData, Vec<OutgoingEdge>);

async fn bookmark_step<V: VisitOne>(
    ctx: CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    b: BookmarkName,
    published_bookmarks: Arc<HashMap<BookmarkName, ChangesetId>>,
) -> Result<StepOutput, Error> {
    let bcs_opt = match published_bookmarks.get(&b) {
        Some(csid) => Some(csid.clone()),
        // Just in case we have non-public bookmarks
        None => repo.get_bonsai_bookmark(ctx, &b).await?,
    };
    match bcs_opt {
        Some(bcs_id) => {
            let mut edges = vec![];
            checker.add_edge(&mut edges, EdgeType::BookmarkToChangeset, || {
                Node::Changeset(bcs_id)
            });
            checker.add_edge(&mut edges, EdgeType::BookmarkToBonsaiHgMapping, || {
                Node::BonsaiHgMapping(bcs_id)
            });
            Ok(StepOutput(
                checker.step_data(NodeType::Bookmark, || NodeData::Bookmark(bcs_id)),
                edges,
            ))
        }
        None => Err(format_err!("Unknown Bookmark {}", b)),
    }
}

async fn published_bookmarks_step<V: VisitOne>(
    published_bookmarks: Arc<HashMap<BookmarkName, ChangesetId>>,
    checker: &Checker<V>,
) -> Result<StepOutput, Error> {
    let mut edges = vec![];
    for (_, bcs_id) in published_bookmarks.iter() {
        checker.add_edge(&mut edges, EdgeType::PublishedBookmarksToChangeset, || {
            Node::Changeset(bcs_id.clone())
        });
        checker.add_edge(
            &mut edges,
            EdgeType::PublishedBookmarksToBonsaiHgMapping,
            || Node::BonsaiHgMapping(bcs_id.clone()),
        );
    }
    Ok(StepOutput(
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
) -> Result<StepOutput, Error> {
    let maybe_phase = if checker.is_public(ctx, bcs_id).await? {
        Some(Phase::Public)
    } else {
        None
    };
    Ok(StepOutput(
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
) -> Result<StepOutput, Error> {
    let blame = blame_id.load(ctx, repo.blobstore()).await?;
    let mut edges = vec![];

    if let BlameMaybeRejected::Blame(blame) = blame {
        for r in blame.ranges() {
            checker.add_edge(&mut edges, EdgeType::BlameToChangeset, || {
                Node::Changeset(r.csid)
            });
        }
        Ok(StepOutput(
            checker.step_data(NodeType::Blame, || NodeData::Blame(Some(blame))),
            edges,
        ))
    } else {
        Ok(StepOutput(
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
) -> Result<StepOutput, Error> {
    let log = id.load(ctx, repo.blobstore()).await?;
    let mut edges = vec![];
    for (cs_id, _offsets) in log.latest() {
        checker.add_edge(&mut edges, EdgeType::FastlogBatchToChangeset, || {
            Node::Changeset(*cs_id)
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
    Ok(StepOutput(
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
) -> Result<StepOutput, Error> {
    let log =
        fetch_fastlog_batch_by_unode_id(ctx, repo.blobstore(), &UnodeManifestEntry::Tree(id.inner))
            .await?;
    let mut edges = vec![];
    if let Some(log) = &log {
        for (cs_id, _offsets) in log.latest() {
            checker.add_edge(&mut edges, EdgeType::FastlogDirToChangeset, || {
                Node::Changeset(*cs_id)
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

    Ok(StepOutput(
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
) -> Result<StepOutput, Error> {
    let log =
        fetch_fastlog_batch_by_unode_id(ctx, repo.blobstore(), &UnodeManifestEntry::Leaf(id.inner))
            .await?;
    let mut edges = vec![];
    if let Some(log) = &log {
        for (cs_id, _offsets) in log.latest() {
            checker.add_edge(&mut edges, EdgeType::FastlogFileToChangeset, || {
                Node::Changeset(*cs_id)
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
    Ok(StepOutput(
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
) -> Result<StepOutput, Error> {
    if is_derived::<ChangesetInfo>(ctx, repo, bcs_id, enable_derive).await? {
        let mut edges = vec![];
        checker.add_edge(
            &mut edges,
            EdgeType::ChangesetInfoMappingToChangesetInfo,
            || Node::ChangesetInfo(bcs_id),
        );
        Ok(StepOutput(
            checker.step_data(NodeType::ChangesetInfoMapping, || {
                NodeData::ChangesetInfoMapping(Some(bcs_id))
            }),
            edges,
        ))
    } else {
        Ok(StepOutput(
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
) -> Result<StepOutput, Error> {
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
        Ok(StepOutput(
            checker.step_data(NodeType::ChangesetInfo, || {
                NodeData::ChangesetInfo(Some(info))
            }),
            edges,
        ))
    } else {
        Ok(StepOutput(
            checker.step_data(NodeType::ChangesetInfo, || NodeData::ChangesetInfo(None)),
            vec![],
        ))
    }
}

async fn bonsai_changeset_step<V: VisitOne>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    bcs_id: &ChangesetId,
) -> Result<StepOutput, Error> {
    // Get the data, and add direct file data for this bonsai changeset
    let bcs = bcs_id.load(ctx, repo.blobstore()).await?;

    // Build edges, from mostly queue expansion to least
    let mut edges = vec![];

    // Expands to parents
    checker.add_edge(&mut edges, EdgeType::ChangesetToChangesetInfoMapping, || {
        Node::ChangesetInfoMapping(*bcs_id)
    });

    // Parents expand 1:[0|1|2] and then the same as all below
    for parent_id in bcs.parents() {
        checker.add_edge(&mut edges, EdgeType::ChangesetToBonsaiParent, || {
            Node::Changeset(parent_id)
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
    // Deleted manifest mapping is 1:1 but from their expands less than unodes
    checker.add_edge(
        &mut edges,
        EdgeType::ChangesetToDeletedManifestMapping,
        || Node::DeletedManifestMapping(*bcs_id),
    );
    // Allow Hg based lookup which is 1:[1|0], may expand a lot from that
    checker.add_edge(&mut edges, EdgeType::ChangesetToBonsaiHgMapping, || {
        Node::BonsaiHgMapping(*bcs_id)
    });
    // File content expands just to meta+aliases 1:~5, with no further steps
    for (mpath, fc) in bcs.file_changes() {
        if let Some(fc) = fc {
            checker.add_edge_with_path(
                &mut edges,
                EdgeType::ChangesetToFileContent,
                || Node::FileContent(fc.content_id()),
                || Some(WrappedPath::from(Some(mpath.clone()))),
            );
        }
    }
    // Phase mapping is 1:[0|1]
    checker.add_edge(&mut edges, EdgeType::ChangesetToPhaseMapping, || {
        Node::PhaseMapping(*bcs_id)
    });

    Ok(StepOutput(
        checker.step_data(NodeType::Changeset, || NodeData::Changeset(bcs)),
        edges,
    ))
}

fn file_content_step<V: VisitOne>(
    ctx: CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    id: ContentId,
) -> Result<StepOutput, Error> {
    let s = filestore::fetch_stream(repo.get_blobstore(), ctx, id).map_ok(FileBytes);
    // We don't force file loading here, content may not be needed
    Ok(StepOutput(
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
) -> Result<StepOutput, Error> {
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
            checker.add_edge(&mut edges, EdgeType::FileContentMetadataToSha256Alias, || {
                Node::AliasContentMapping(AliasKey(Alias::Sha256(metadata.sha256)))
            });
            checker.add_edge(
                &mut edges,
                EdgeType::FileContentMetadataToGitSha1Alias,
                || Node::AliasContentMapping(AliasKey(Alias::GitSha1(metadata.git_sha1.sha1()))),
            );
            Ok(StepOutput(
                checker.step_data(NodeType::FileContentMetadata, || {
                    NodeData::FileContentMetadata(Some(metadata))
                }),
                edges,
            ))
        }
        Some(None) | None => Ok(StepOutput(
            checker.step_data(NodeType::FileContentMetadata, || {
                NodeData::FileContentMetadata(None)
            }),
            vec![],
        )),
    }
}

async fn bonsai_to_hg_mapping_step<'a, V: 'a + VisitOne>(
    ctx: &'a CoreContext,
    repo: &'a BlobRepo,
    checker: &'a Checker<V>,
    bcs_id: ChangesetId,
    enable_derive: bool,
) -> Result<StepOutput, Error> {
    let has_filenode = if enable_derive {
        if checker.is_public(ctx, &bcs_id).await? {
            let _ = FilenodesOnlyPublic::derive(ctx, repo, bcs_id).await?;
            Some(true)
        } else {
            None
        }
    } else {
        None
    };

    // We only want to walk to Hg step if filenode is present
    let has_filenode = match has_filenode {
        Some(v) => v,
        None => FilenodesOnlyPublic::is_derived(&ctx, &repo, &bcs_id).await?,
    };

    let maybe_hg_cs_id = if has_filenode {
        maybe_derived::<MappedHgChangesetId>(ctx, repo, bcs_id, enable_derive).await?
    } else {
        None
    };

    Ok(match maybe_hg_cs_id {
        Some(hg_cs_id) => {
            let hg_cs_id = hg_cs_id.0;
            let mut edges = vec![];
            checker.add_edge(&mut edges, EdgeType::BonsaiHgMappingToHgChangeset, || {
                Node::HgChangeset(hg_cs_id)
            });
            StepOutput(
                checker.step_data(NodeType::BonsaiHgMapping, || {
                    NodeData::BonsaiHgMapping(Some(hg_cs_id))
                }),
                edges,
            )
        }
        None => StepOutput(
            checker.step_data(NodeType::BonsaiHgMapping, || {
                NodeData::BonsaiHgMapping(None)
            }),
            vec![],
        ),
    })
}

async fn hg_to_bonsai_mapping_step<V: VisitOne>(
    ctx: CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    id: HgChangesetId,
) -> Result<StepOutput, Error> {
    let maybe_bcs_id = repo.get_bonsai_from_hg(ctx, id).compat().await?;
    match maybe_bcs_id {
        Some(bcs_id) => {
            let mut edges = vec![];
            checker.add_edge(&mut edges, EdgeType::HgBonsaiMappingToChangeset, || {
                Node::Changeset(bcs_id)
            });
            Ok(StepOutput(
                checker.step_data(NodeType::HgBonsaiMapping, || {
                    NodeData::HgBonsaiMapping(Some(bcs_id))
                }),
                edges,
            ))
        }
        None => Ok(StepOutput(
            checker.step_data(NodeType::HgBonsaiMapping, || {
                NodeData::HgBonsaiMapping(None)
            }),
            vec![],
        )),
    }
}

async fn hg_changeset_step<V: VisitOne>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    id: HgChangesetId,
) -> Result<StepOutput, Error> {
    let hgchangeset = id.load(ctx, repo.blobstore()).await?;
    let mut edges = vec![];
    // 1:1 but will then expand a lot, usually
    checker.add_edge(&mut edges, EdgeType::HgChangesetToHgManifest, || {
        Node::HgManifest(PathKey::new(hgchangeset.manifestid(), WrappedPath::Root))
    });
    // Mostly 1:1, can be 1:2, with further expansion
    for p in hgchangeset.parents().into_iter() {
        checker.add_edge(&mut edges, EdgeType::HgChangesetToHgParent, || {
            Node::HgChangeset(HgChangesetId::new(p))
        });
    }
    Ok(StepOutput(
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
) -> Result<StepOutput, Error> {
    let envelope = hg_file_node_id.load(ctx, repo.blobstore()).await?;
    let mut edges = vec![];
    checker.add_edge_with_path(
        &mut edges,
        EdgeType::HgFileEnvelopeToFileContent,
        || Node::FileContent(envelope.content_id()),
        || path.cloned(),
    );
    Ok(StepOutput(
        checker.step_data(NodeType::HgFileEnvelope, || {
            NodeData::HgFileEnvelope(envelope)
        }),
        edges,
    ))
}

async fn hg_file_node_step<V: VisitOne>(
    ctx: CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    path: WrappedPath,
    hg_file_node_id: HgFileNodeId,
) -> Result<StepOutput, Error> {
    let repo_path = match &path {
        WrappedPath::Root => RepoPath::RootPath,
        WrappedPath::NonRoot(path) => RepoPath::FilePath(path.mpath().clone()),
    };
    let file_node_opt = repo
        .get_filenode_opt(ctx, &repo_path, hg_file_node_id)
        .and_then(|filenode| filenode.do_not_handle_disabled_filenodes())
        .compat()
        .await?;
    match file_node_opt {
        Some(file_node_info) => {
            let mut edges = vec![];
            // Validate hg link node
            checker.add_edge(&mut edges, EdgeType::HgFileNodeToLinkedHgChangeset, || {
                Node::HgChangeset(file_node_info.linknode)
            });

            // Following linknode bonsai increases parallelism of walk.
            // Linknodes will point to many commits we can then walk
            // in parallel
            checker.add_edge(
                &mut edges,
                EdgeType::HgFileNodeToLinkedHgBonsaiMapping,
                || Node::HgBonsaiMapping(file_node_info.linknode),
            );

            // Parents
            for parent in &[file_node_info.p1, file_node_info.p2] {
                if let Some(parent) = parent {
                    checker.add_edge(&mut edges, EdgeType::HgFileNodeToHgParentFileNode, || {
                        Node::HgFileNode(PathKey::new(*parent, path.clone()))
                    })
                }
            }

            // Copyfrom is like another parent
            for (repo_path, file_node_id) in &file_node_info.copyfrom {
                checker.add_edge(&mut edges, EdgeType::HgFileNodeToHgCopyfromFileNode, || {
                    Node::HgFileNode(PathKey::new(
                        *file_node_id,
                        WrappedPath::from(repo_path.clone().into_mpath()),
                    ))
                })
            }
            Ok(StepOutput(
                checker.step_data(NodeType::HgFileNode, || {
                    NodeData::HgFileNode(Some(file_node_info))
                }),
                edges,
            ))
        }
        None => Ok(StepOutput(
            checker.step_data(NodeType::HgFileNode, || NodeData::HgFileNode(None)),
            vec![],
        )),
    }
}

async fn hg_manifest_step<V: VisitOne>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    path: WrappedPath,
    hg_manifest_id: HgManifestId,
) -> Result<StepOutput, Error> {
    let hgmanifest = hg_manifest_id.load(ctx, repo.blobstore()).await?;
    let (manifests, filenodes): (Vec<_>, Vec<_>) =
        hgmanifest.list().partition_map(|(name, entry)| {
            let path_opt = WrappedPath::from(Some(MPath::join_opt_element(path.as_ref(), &name)));
            match entry {
                Entry::Leaf((_, filenode_id)) => Either::Right((path_opt, filenode_id)),
                Entry::Tree(manifest_id) => Either::Left((path_opt, manifest_id)),
            }
        });
    let mut edges = vec![];
    // Manifests expand as a tree so 1:N
    for (full_path, hg_child_manifest_id) in manifests {
        checker.add_edge(&mut edges, EdgeType::HgManifestToChildHgManifest, || {
            Node::HgManifest(PathKey::new(hg_child_manifest_id, full_path))
        })
    }

    let mut filenode_edges = vec![];
    let mut envelope_edges = vec![];
    for (full_path, hg_file_node_id) in filenodes {
        checker.add_edge_with_path(
            &mut envelope_edges,
            EdgeType::HgManifestToHgFileEnvelope,
            || Node::HgFileEnvelope(hg_file_node_id),
            || Some(full_path.clone()),
        );
        checker.add_edge(&mut filenode_edges, EdgeType::HgManifestToHgFileNode, || {
            Node::HgFileNode(PathKey::new(hg_file_node_id, full_path))
        });
    }
    // File nodes can expand a lot into history via linknodes
    edges.append(&mut filenode_edges);
    // Envelopes expand 1:1 to file content
    edges.append(&mut envelope_edges);

    Ok(StepOutput(
        checker.step_data(NodeType::HgManifest, || NodeData::HgManifest(hgmanifest)),
        edges,
    ))
}

async fn alias_content_mapping_step<V: VisitOne>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    alias: Alias,
) -> Result<StepOutput, Error> {
    let content_id = alias.load(ctx, repo.blobstore()).await?;
    let mut edges = vec![];
    checker.add_edge(&mut edges, EdgeType::AliasContentMappingToFileContent, || {
        Node::FileContent(content_id)
    });
    Ok(StepOutput(
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
        Ok(Derived::is_derived(&ctx, &repo, &bcs_id).await?)
    }
}

async fn bonsai_to_fsnode_mapping_step<V: VisitOne>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    bcs_id: ChangesetId,
    enable_derive: bool,
) -> Result<StepOutput, Error> {
    let root_fsnode_id = maybe_derived::<RootFsnodeId>(ctx, repo, bcs_id, enable_derive).await?;

    if let Some(root_fsnode_id) = root_fsnode_id {
        let mut edges = vec![];
        checker.add_edge_with_path(
            &mut edges,
            EdgeType::FsnodeMappingToRootFsnode,
            || Node::Fsnode(*root_fsnode_id.fsnode_id()),
            || Some(WrappedPath::Root),
        );
        Ok(StepOutput(
            checker.step_data(NodeType::FsnodeMapping, || {
                NodeData::FsnodeMapping(Some(*root_fsnode_id.fsnode_id()))
            }),
            edges,
        ))
    } else {
        Ok(StepOutput(
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
) -> Result<StepOutput, Error> {
    let fsnode = fsnode_id
        .load(ctx, &repo.get_blobstore())
        .map_err(Error::from)
        .await?;

    let mut content_edges = vec![];
    let mut dir_edges = vec![];
    for (child, fsnode_entry) in fsnode.list() {
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

    // Ordering to reduce queue depth
    dir_edges.append(&mut content_edges);

    Ok(StepOutput(
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
) -> Result<StepOutput, Error> {
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
        Ok(StepOutput(
            checker.step_data(NodeType::UnodeMapping, || {
                NodeData::UnodeMapping(Some(manifest_id))
            }),
            edges,
        ))
    } else {
        Ok(StepOutput(
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
) -> Result<StepOutput, Error> {
    let unode_file = key.inner.load(ctx, repo.blobstore()).await?;
    let linked_cs_id = *unode_file.linknode();
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
        Node::Changeset(linked_cs_id)
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

    Ok(StepOutput(
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
) -> Result<StepOutput, Error> {
    let unode_manifest = key.inner.load(ctx, repo.blobstore()).await?;
    let linked_cs_id = *unode_manifest.linknode();

    let mut edges = vec![];

    checker.add_edge(&mut edges, EdgeType::UnodeManifestToLinkedChangeset, || {
        Node::Changeset(linked_cs_id)
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

    Ok(StepOutput(
        checker.step_data(NodeType::UnodeManifest, || {
            NodeData::UnodeManifest(unode_manifest)
        }),
        edges,
    ))
}

async fn deleted_manifest_step<V: VisitOne>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    id: &DeletedManifestId,
    path: Option<&WrappedPath>,
) -> Result<StepOutput, Error> {
    let deleted_manifest = id.load(ctx, repo.blobstore()).await?;
    let linked_cs_id = *deleted_manifest.linknode();

    let mut edges = vec![];

    if let Some(linked_cs_id) = linked_cs_id {
        checker.add_edge(&mut edges, EdgeType::DeletedManifestToLinkedChangeset, || {
            Node::Changeset(linked_cs_id)
        });
    }

    for (child_path, deleted_manifest_id) in deleted_manifest.list() {
        checker.add_edge_with_path(
            &mut edges,
            EdgeType::DeletedManifestToDeletedManifestChild,
            || Node::DeletedManifest(*deleted_manifest_id),
            || {
                path.map(|p| {
                    WrappedPath::from(MPath::join_element_opt(p.as_ref(), Some(child_path)))
                })
            },
        );
    }

    Ok(StepOutput(
        checker.step_data(NodeType::DeletedManifest, || {
            NodeData::DeletedManifest(Some(deleted_manifest))
        }),
        edges,
    ))
}

async fn deleted_manifest_mapping_step<V: VisitOne>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    bcs_id: ChangesetId,
    enable_derive: bool,
) -> Result<StepOutput, Error> {
    let root_manifest_id =
        maybe_derived::<RootDeletedManifestId>(ctx, repo, bcs_id, enable_derive).await?;

    if let Some(root_manifest_id) = root_manifest_id {
        let mut edges = vec![];
        checker.add_edge_with_path(
            &mut edges,
            EdgeType::DeletedManifestMappingToRootDeletedManifest,
            || Node::DeletedManifest(*root_manifest_id.deleted_manifest_id()),
            || Some(WrappedPath::Root),
        );
        Ok(StepOutput(
            checker.step_data(NodeType::DeletedManifestMapping, || {
                NodeData::DeletedManifestMapping(Some(*root_manifest_id.deleted_manifest_id()))
            }),
            edges,
        ))
    } else {
        Ok(StepOutput(
            checker.step_data(NodeType::DeletedManifestMapping, || {
                NodeData::DeletedManifestMapping(None)
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
) -> Result<StepOutput, Error> {
    let manifest = manifest_id.load(ctx, repo.blobstore()).await?;
    let mut edges = vec![];

    for (child_path, entry) in manifest.list() {
        match entry {
            SkeletonManifestEntry::Directory(subdir) => {
                checker.add_edge_with_path(
                    &mut edges,
                    EdgeType::SkeletonManifestToSkeletonManifestChild,
                    || Node::SkeletonManifest(*subdir.id()),
                    || {
                        path.map(|p| {
                            WrappedPath::from(MPath::join_element_opt(p.as_ref(), Some(child_path)))
                        })
                    },
                );
            }
            SkeletonManifestEntry::File => {}
        }
    }

    Ok(StepOutput(
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
) -> Result<StepOutput, Error> {
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
        Ok(StepOutput(
            checker.step_data(NodeType::SkeletonManifestMapping, || {
                NodeData::SkeletonManifestMapping(Some(*root_manifest_id.skeleton_manifest_id()))
            }),
            edges,
        ))
    } else {
        Ok(StepOutput(
            checker.step_data(NodeType::SkeletonManifestMapping, || {
                NodeData::SkeletonManifestMapping(None)
            }),
            vec![],
        ))
    }
}

/// Expand nodes where check for a type is used as a check for other types.
/// e.g. to make sure metadata looked up/considered for files.
pub fn expand_checked_nodes(children: &mut Vec<OutgoingEdge>) -> () {
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
    always_emit_edge_types: HashSet<EdgeType>,
    required_node_data_types: HashSet<NodeType>,
    keep_edge_paths: bool,
    visitor: V,
    phases_store: Arc<dyn Phases>,
    with_blame: bool,
    with_fastlog: bool,
}

impl<V: VisitOne> Checker<V> {
    async fn is_public(&self, ctx: &CoreContext, bcs_id: &ChangesetId) -> Result<bool, Error> {
        self.visitor
            .is_public(ctx, self.phases_store.as_ref(), bcs_id)
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

/// Walk the graph from one or more starting points,  providing stream of data for later reduction
pub fn walk_exact<V, VOut, Route>(
    ctx: CoreContext,
    repo: BlobRepo,
    enable_derive: bool,
    walk_roots: Vec<OutgoingEdge>,
    visitor: V,
    scheduled_max: usize,
    error_as_data_node_types: HashSet<NodeType>,
    error_as_data_edge_types: HashSet<EdgeType>,
    include_edge_types: HashSet<EdgeType>,
    always_emit_edge_types: HashSet<EdgeType>,
    required_node_data_types: HashSet<NodeType>,
    scuba: MononokeScubaSampleBuilder,
    keep_edge_paths: bool,
) -> BoxStream<'static, Result<VOut, Error>>
where
    V: 'static + Clone + WalkVisitor<VOut, Route> + Send + Sync,
    VOut: 'static + Send,
    Route: 'static + Send + Clone + StepRoute,
{
    // Build lookups
    let published_bookmarks = repo
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
    let walk_roots: Vec<(Option<Route>, OutgoingEdge)> =
        walk_roots.into_iter().map(|e| (None, e)).collect();

    published_bookmarks
        .map_ok(move |published_bookmarks| {
            let published_bookmarks = Arc::new(published_bookmarks);

            let heads_fetcher: HeadsFetcher = Arc::new({
                cloned!(published_bookmarks);
                move |_ctx: &CoreContext| {
                    future::ok(
                        published_bookmarks
                            .iter()
                            .map(|(_, csid)| csid)
                            .cloned()
                            .collect(),
                    )
                    .boxed()
                }
            });

            let checker = Arc::new(Checker {
                with_blame: include_edge_types
                    .iter()
                    .any(|e| e.outgoing_type() == NodeType::Blame),
                with_fastlog: include_edge_types
                    .iter()
                    .any(|e| e.outgoing_type().derived_data_name() == Some(RootFastlog::NAME)),
                include_edge_types,
                always_emit_edge_types,
                keep_edge_paths,
                visitor: visitor.clone(),
                required_node_data_types,
                phases_store: repo.get_phases_factory().get_phases(
                    repo.get_repoid(),
                    repo.get_changeset_fetcher(),
                    heads_fetcher,
                ),
            });

            bounded_traversal_stream(scheduled_max, walk_roots, {
                move |(via, walk_item): (Option<Route>, OutgoingEdge)| {
                    let ctx = visitor.start_step(ctx.clone(), via.as_ref(), &walk_item);
                    cloned!(
                        error_as_data_node_types,
                        error_as_data_edge_types,
                        published_bookmarks,
                        repo,
                        scuba,
                        visitor,
                        checker,
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
                            scuba,
                            published_bookmarks,
                            checker,
                        );

                        let handle = tokio::task::spawn(next);
                        handle.await?
                    }
                    .boxed()
                }
            })
        })
        .try_flatten_stream()
        .boxed()
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
    (
        VOut,
        impl IntoIterator<Item = (Option<Route>, OutgoingEdge)>,
    ),
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
        Node::Root(_) => Err(format_err!("Not expecting Roots to be generated")),
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
        Node::Changeset(bcs_id) => bonsai_changeset_step(&ctx, &repo, &checker, &bcs_id).await,
        Node::BonsaiHgMapping(bcs_id) => {
            bonsai_to_hg_mapping_step(&ctx, &repo, &checker, bcs_id, enable_derive).await
        }
        Node::PhaseMapping(bcs_id) => bonsai_phase_step(&ctx, &checker, &bcs_id).await,
        Node::PublishedBookmarks(_) => {
            published_bookmarks_step(published_bookmarks.clone(), &checker).await
        }
        // Hg
        Node::HgBonsaiMapping(hg_csid) => {
            hg_to_bonsai_mapping_step(ctx.clone(), &repo, &checker, hg_csid).await
        }
        Node::HgChangeset(hg_csid) => hg_changeset_step(&ctx, &repo, &checker, hg_csid).await,
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
        Node::HgManifest(PathKey { id, path }) => {
            hg_manifest_step(&ctx, &repo, &checker, path, id).await
        }
        // Content
        Node::FileContent(content_id) => {
            file_content_step(ctx.clone(), &repo, &checker, content_id)
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
        Node::DeletedManifest(id) => {
            deleted_manifest_step(&ctx, &repo, &checker, &id, walk_item.path.as_ref()).await
        }
        Node::DeletedManifestMapping(bcs_id) => {
            deleted_manifest_mapping_step(&ctx, &repo, &checker, bcs_id, enable_derive).await
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
                &mut scuba,
            );
            scuba
                .add(EDGE_TYPE, Into::<&'static str>::into(edge_label))
                .add(CHECK_TYPE, "step")
                .add(CHECK_FAIL, 1)
                .add("error_msg", msg.clone())
                .log();
            // Optionally attempt to continue
            if error_as_data_node_types.contains(&walk_item.target.get_type()) {
                if error_as_data_edge_types.is_empty()
                    || error_as_data_edge_types.contains(&walk_item.label)
                {
                    warn!(logger, "{}", msg);
                    Ok(StepOutput(
                        NodeData::ErrorAsData(walk_item.target.clone()),
                        vec![],
                    ))
                } else {
                    Err(e)
                }
            } else {
                Err(e)
            }
        }
    }
    .with_context(|| ErrorKind::NotTraversable(walk_item.clone(), format!("{:?}", via)))?;

    match step_output {
        StepOutput(node_data, children) => {
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
            Ok(visitor.visit(&ctx, walk_item, Some(node_data), via, children)).map(
                |(vout, via, next)| {
                    let via = Some(via);
                    let next = next.into_iter().map(move |e| (via.clone(), e));
                    (vout, next)
                },
            )
        }
    }
}
