/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::graph::{EdgeType, FileContentData, Node, NodeData, NodeType, WrappedPath};
use crate::validate::{add_node_to_scuba, CHECK_FAIL, CHECK_TYPE, EDGE_TYPE};

use anyhow::{format_err, Context, Error};
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use bookmarks::{BookmarkKind, BookmarkName, BookmarkPagination, BookmarkPrefix, Freshness};
use bounded_traversal::bounded_traversal_stream;
use cloned::cloned;
use context::CoreContext;
use derived_data::BonsaiDerived;
use derived_data_filenodes::FilenodesOnlyPublic;
use filestore::{self, Alias};
use fsnodes::RootFsnodeId;
use futures::{
    compat::{Future01CompatExt, Stream01CompatExt},
    future::{self, Future, FutureExt, TryFutureExt},
    stream::{BoxStream, StreamExt, TryStreamExt},
};
use futures_ext::FutureExt as Future01Ext;
use futures_old::{future as old_future, Future as Future01, Stream as Stream01};
use itertools::{Either, Itertools};
use mercurial_types::{
    FileBytes, HgChangesetId, HgEntryId, HgFileNodeId, HgManifest, HgManifestId, RepoPath,
};
use mononoke_types::{fsnode::FsnodeEntry, ChangesetId, ContentId, FsnodeId, MPath};
use phases::{HeadsFetcher, Phase, Phases};
use scuba_ext::ScubaSampleBuilder;
use slog::warn;
use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    iter::{IntoIterator, Iterator},
    sync::Arc,
};
use thiserror::Error;

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
pub trait VisitOne {
    fn needs_visit(&self, outgoing: &OutgoingEdge) -> bool;
}

// Overall trait with support for route tracking and handling
// partially derived types (it can see the node_data)
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

impl<V> VisitOne for Arc<V>
where
    V: VisitOne,
{
    fn needs_visit(&self, outgoing: &OutgoingEdge) -> bool {
        self.as_ref().needs_visit(outgoing)
    }
}

impl<V, VOut, Route> WalkVisitor<VOut, Route> for Arc<V>
where
    V: 'static + WalkVisitor<VOut, Route> + Sync + Send,
    VOut: Send + 'static,
    Route: Send + 'static,
{
    fn start_step(
        &self,
        ctx: CoreContext,
        route: Option<&Route>,
        step: &OutgoingEdge,
    ) -> CoreContext {
        self.as_ref().start_step(ctx, route, step)
    }

    fn visit(
        &self,
        ctx: &CoreContext,
        resolved: OutgoingEdge,
        node_data: Option<NodeData>,
        route: Option<Route>,
        outgoing: Vec<OutgoingEdge>,
    ) -> (VOut, Route, Vec<OutgoingEdge>) {
        self.as_ref()
            .visit(ctx, resolved, node_data, route, outgoing)
    }
}

// Data found for this node, plus next steps
struct StepOutput(NodeData, Vec<OutgoingEdge>);

fn bookmark_step<'a, V: VisitOne>(
    ctx: CoreContext,
    repo: &'a BlobRepo,
    checker: &'a Checker<V>,
    b: BookmarkName,
    published_bookmarks: Arc<HashMap<BookmarkName, ChangesetId>>,
) -> impl Future<Output = Result<StepOutput, Error>> + 'a {
    match published_bookmarks.get(&b) {
        Some(csid) => future::ok(Some(csid.clone())).left_future(),
        // Just in case we have non-public bookmarks
        None => repo.get_bonsai_bookmark(ctx, &b).compat().right_future(),
    }
    .and_then(move |bcs_opt| match bcs_opt {
        Some(bcs_id) => {
            let mut edges = vec![];
            checker.add_edge(&mut edges, EdgeType::BookmarkToBonsaiChangeset, || {
                Node::BonsaiChangeset(bcs_id)
            });
            checker.add_edge(&mut edges, EdgeType::BookmarkToBonsaiHgMapping, || {
                Node::BonsaiHgMapping(bcs_id)
            });
            future::ok(StepOutput(
                checker.step_data(NodeType::Bookmark, || NodeData::Bookmark(bcs_id)),
                edges,
            ))
        }
        None => future::err(format_err!("Unknown Bookmark {}", b)),
    })
}

fn published_bookmarks_step<V: VisitOne>(
    published_bookmarks: Arc<HashMap<BookmarkName, ChangesetId>>,
    checker: &Checker<V>,
) -> impl Future<Output = Result<StepOutput, Error>> {
    let mut edges = vec![];
    for (_, bcs_id) in published_bookmarks.iter() {
        checker.add_edge(
            &mut edges,
            EdgeType::PublishedBookmarksToBonsaiChangeset,
            || Node::BonsaiChangeset(bcs_id.clone()),
        );
        checker.add_edge(
            &mut edges,
            EdgeType::PublishedBookmarksToBonsaiHgMapping,
            || Node::BonsaiHgMapping(bcs_id.clone()),
        );
    }
    future::ok(StepOutput(
        checker.step_data(NodeType::PublishedBookmarks, || {
            NodeData::PublishedBookmarks
        }),
        edges,
    ))
}

fn bonsai_phase_step<'a, V: VisitOne>(
    ctx: &'a CoreContext,
    checker: &'a Checker<V>,
    phases_store: Arc<dyn Phases>,
    bcs_id: ChangesetId,
) -> impl Future<Output = Result<StepOutput, Error>> + 'a {
    phases_store
        .get_public(ctx.clone(), vec![bcs_id], true)
        .map(move |public| public.contains(&bcs_id))
        .compat()
        .map_ok(move |is_public| {
            let phase = if is_public { Some(Phase::Public) } else { None };
            StepOutput(
                checker.step_data(NodeType::BonsaiPhaseMapping, || {
                    NodeData::BonsaiPhaseMapping(phase)
                }),
                vec![],
            )
        })
}

async fn bonsai_changeset_step<V: VisitOne>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    bcs_id: &ChangesetId,
) -> Result<StepOutput, Error> {
    // Get the data, and add direct file data for this bonsai changeset
    let bcs = bcs_id.load(ctx.clone(), repo.blobstore()).await?;

    let mut edges = vec![];

    // Parents deliberately first to resolve dependent reads as early as possible
    for parent_id in bcs.parents() {
        checker.add_edge(&mut edges, EdgeType::BonsaiChangesetToBonsaiParent, || {
            Node::BonsaiChangeset(parent_id)
        });
    }
    // Allow Hg based lookup
    checker.add_edge(
        &mut edges,
        EdgeType::BonsaiChangesetToBonsaiHgMapping,
        || Node::BonsaiHgMapping(*bcs_id),
    );
    checker.add_edge(
        &mut edges,
        EdgeType::BonsaiChangesetToBonsaiPhaseMapping,
        || Node::BonsaiPhaseMapping(*bcs_id),
    );
    for (mpath, fc) in bcs.file_changes() {
        if let Some(fc) = fc {
            checker.add_edge_with_path(
                &mut edges,
                EdgeType::BonsaiChangesetToFileContent,
                || Node::FileContent(fc.content_id()),
                || Some(WrappedPath::from(Some(mpath.clone()))),
            );
        }
    }
    checker.add_edge(
        &mut edges,
        EdgeType::BonsaiChangesetToBonsaiFsnodeMapping,
        || Node::BonsaiFsnodeMapping(*bcs_id),
    );
    Ok(StepOutput(
        checker.step_data(NodeType::BonsaiChangeset, || NodeData::BonsaiChangeset(bcs)),
        edges,
    ))
}

fn file_content_step<V: VisitOne>(
    ctx: CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    id: ContentId,
) -> Result<StepOutput, Error> {
    let s = filestore::fetch_stream(repo.blobstore(), ctx, id)
        .map(FileBytes)
        .compat();
    // We don't force file loading here, content may not be needed
    Ok(StepOutput(
        checker.step_data(NodeType::FileContent, || {
            NodeData::FileContent(FileContentData::ContentStream(Box::pin(s)))
        }),
        vec![],
    ))
}

fn file_content_metadata_step<'a, V: VisitOne>(
    ctx: CoreContext,
    repo: &'a BlobRepo,
    checker: &'a Checker<V>,
    id: ContentId,
    enable_derive: bool,
) -> impl Future<Output = Result<StepOutput, Error>> + 'a {
    let loader = if enable_derive {
        filestore::get_metadata(repo.blobstore(), ctx, &id.into())
            .map(Some)
            .left_future()
    } else {
        filestore::get_metadata_readonly(repo.blobstore(), ctx, &id.into()).right_future()
    };

    loader
        .map(move |metadata_opt| match metadata_opt {
            Some(Some(metadata)) => {
                let mut edges = vec![];
                checker.add_edge(&mut edges, EdgeType::FileContentMetadataToSha1Alias, || {
                    Node::AliasContentMapping(Alias::Sha1(metadata.sha1))
                });
                checker.add_edge(
                    &mut edges,
                    EdgeType::FileContentMetadataToSha256Alias,
                    || Node::AliasContentMapping(Alias::Sha256(metadata.sha256)),
                );
                checker.add_edge(
                    &mut edges,
                    EdgeType::FileContentMetadataToGitSha1Alias,
                    || Node::AliasContentMapping(Alias::GitSha1(metadata.git_sha1.sha1())),
                );
                StepOutput(
                    checker.step_data(NodeType::FileContentMetadata, || {
                        NodeData::FileContentMetadata(Some(metadata))
                    }),
                    edges,
                )
            }
            Some(None) | None => StepOutput(
                checker.step_data(NodeType::FileContentMetadata, || {
                    NodeData::FileContentMetadata(None)
                }),
                vec![],
            ),
        })
        .compat()
}

fn bonsai_to_hg_mapping_step<'a, V: 'a + VisitOne>(
    ctx: &'a CoreContext,
    repo: &'a BlobRepo,
    checker: &'a Checker<V>,
    bcs_id: ChangesetId,
    enable_derive: bool,
) -> impl Future<Output = Result<StepOutput, Error>> + 'a {
    let hg_cs_id = if enable_derive {
        let filenodes_derive = repo
            .get_phases()
            .get_public(ctx.clone(), vec![bcs_id], false /* ephemeral_derive */)
            .and_then({
                cloned!(ctx, repo);
                move |public| {
                    if public.contains(&bcs_id) {
                        FilenodesOnlyPublic::derive(ctx.clone(), repo.clone(), bcs_id)
                            .from_err()
                            .map(|_| ())
                            .left_future()
                    } else {
                        old_future::ok(()).right_future()
                    }
                }
            });

        let hg_cs_derive = repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id);

        filenodes_derive
            .join(hg_cs_derive)
            .map(|((), hg_cs_id)| Some(hg_cs_id))
            .left_future()
    } else {
        // Check that both filenodes and hg changesets are derived
        {
            async move {
                FilenodesOnlyPublic::is_derived(&ctx, &repo, &bcs_id)
                    .map_err(Error::from)
                    .await
            }
        }
        .boxed()
        .compat()
        .join(repo.get_bonsai_hg_mapping().get_hg_from_bonsai(
            ctx.clone(),
            repo.get_repoid(),
            bcs_id,
        ))
        .map(|(filenodes_derived, maybe_hg_cs_id)| {
            if filenodes_derived {
                maybe_hg_cs_id
            } else {
                None
            }
        })
        .right_future()
    };

    hg_cs_id
        .map(move |maybe_hg_cs_id| match maybe_hg_cs_id {
            Some(hg_cs_id) => {
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
        .compat()
}

fn hg_to_bonsai_mapping_step<'a, V: VisitOne>(
    ctx: CoreContext,
    repo: &'a BlobRepo,
    checker: &'a Checker<V>,
    id: HgChangesetId,
) -> impl Future<Output = Result<StepOutput, Error>> + 'a {
    repo.get_bonsai_from_hg(ctx, id)
        .map(move |maybe_bcs_id| match maybe_bcs_id {
            Some(bcs_id) => {
                let mut edges = vec![];
                checker.add_edge(
                    &mut edges,
                    EdgeType::HgBonsaiMappingToBonsaiChangeset,
                    || Node::BonsaiChangeset(bcs_id),
                );
                StepOutput(
                    checker.step_data(NodeType::HgBonsaiMapping, || {
                        NodeData::HgBonsaiMapping(Some(bcs_id))
                    }),
                    edges,
                )
            }
            None => StepOutput(
                checker.step_data(NodeType::HgBonsaiMapping, || {
                    NodeData::HgBonsaiMapping(None)
                }),
                vec![],
            ),
        })
        .compat()
}

fn hg_changeset_step<'a, V: VisitOne>(
    ctx: CoreContext,
    repo: &'a BlobRepo,
    checker: &'a Checker<V>,
    id: HgChangesetId,
) -> impl Future<Output = Result<StepOutput, Error>> + 'a {
    id.load(ctx, repo.blobstore())
        .map_err(Error::from)
        .map_ok(move |hgchangeset| {
            let mut edges = vec![];
            checker.add_edge(&mut edges, EdgeType::HgChangesetToHgManifest, || {
                Node::HgManifest((WrappedPath::Root, hgchangeset.manifestid()))
            });
            for p in hgchangeset.parents().into_iter() {
                checker.add_edge(&mut edges, EdgeType::HgChangesetToHgParent, || {
                    Node::HgChangeset(HgChangesetId::new(p))
                });
            }
            StepOutput(
                checker.step_data(NodeType::HgChangeset, || NodeData::HgChangeset(hgchangeset)),
                edges,
            )
        })
}

fn hg_file_envelope_step<'a, V: 'a + VisitOne>(
    ctx: CoreContext,
    repo: &'a BlobRepo,
    checker: &'a Checker<V>,
    hg_file_node_id: HgFileNodeId,
    path: Option<&'a WrappedPath>,
) -> impl Future<Output = Result<StepOutput, Error>> + 'a {
    hg_file_node_id
        .load(ctx, repo.blobstore())
        .map_err(Error::from)
        .map_ok({
            move |envelope| {
                let mut edges = vec![];
                checker.add_edge_with_path(
                    &mut edges,
                    EdgeType::HgFileEnvelopeToFileContent,
                    || Node::FileContent(envelope.content_id()),
                    || path.cloned(),
                );
                StepOutput(
                    checker.step_data(NodeType::HgFileEnvelope, || {
                        NodeData::HgFileEnvelope(envelope)
                    }),
                    edges,
                )
            }
        })
}

fn hg_file_node_step<'a, V: VisitOne>(
    ctx: CoreContext,
    repo: &'a BlobRepo,
    checker: &'a Checker<V>,
    path: WrappedPath,
    hg_file_node_id: HgFileNodeId,
) -> impl Future<Output = Result<StepOutput, Error>> + 'a {
    let repo_path = match &path {
        WrappedPath::Root => RepoPath::RootPath,
        WrappedPath::NonRoot(path) => RepoPath::FilePath(path.mpath().clone()),
    };
    repo.get_filenode_opt(ctx, &repo_path, hg_file_node_id)
        .and_then(|filenode| filenode.do_not_handle_disabled_filenodes())
        .map(move |file_node_opt| match file_node_opt {
            Some(file_node_info) => {
                let mut edges = vec![];
                // Validate hg link node
                checker.add_edge(&mut edges, EdgeType::HgLinkNodeToHgChangeset, || {
                    Node::HgChangeset(file_node_info.linknode)
                });

                // Following linknode bonsai increases parallelism of walk.
                // Linknodes will point to many commits we can then walk
                // in parallel
                checker.add_edge(&mut edges, EdgeType::HgLinkNodeToHgBonsaiMapping, || {
                    Node::HgBonsaiMapping(file_node_info.linknode)
                });

                // Parents
                for parent in &[file_node_info.p1, file_node_info.p2] {
                    if let Some(parent) = parent {
                        checker.add_edge(&mut edges, EdgeType::HgFileNodeToHgParentFileNode, || {
                            Node::HgFileNode((path.clone(), *parent))
                        })
                    }
                }

                // Copyfrom is like another parent
                for (repo_path, file_node_id) in &file_node_info.copyfrom {
                    checker.add_edge(&mut edges, EdgeType::HgFileNodeToHgCopyfromFileNode, || {
                        Node::HgFileNode((
                            WrappedPath::from(repo_path.clone().into_mpath()),
                            *file_node_id,
                        ))
                    })
                }
                StepOutput(
                    checker.step_data(NodeType::HgFileNode, || {
                        NodeData::HgFileNode(Some(file_node_info))
                    }),
                    edges,
                )
            }
            None => StepOutput(
                checker.step_data(NodeType::HgFileNode, || NodeData::HgFileNode(None)),
                vec![],
            ),
        })
        .compat()
}

fn hg_manifest_step<'a, V: VisitOne>(
    ctx: CoreContext,
    repo: &'a BlobRepo,
    checker: &'a Checker<V>,
    path: WrappedPath,
    hg_manifest_id: HgManifestId,
) -> impl Future<Output = Result<StepOutput, Error>> + 'a {
    hg_manifest_id
        .load(ctx, repo.blobstore())
        .map_err(Error::from)
        .map_ok(move |hgmanifest| {
            let (manifests, filenodes): (Vec<_>, Vec<_>) =
                hgmanifest.list().partition_map(|child| {
                    let path_opt =
                        WrappedPath::from(MPath::join_element_opt(path.as_ref(), child.get_name()));
                    match child.get_hash() {
                        HgEntryId::File(_, filenode_id) => Either::Right((path_opt, filenode_id)),
                        HgEntryId::Manifest(manifest_id) => Either::Left((path_opt, manifest_id)),
                    }
                });
            let mut edges = vec![];
            for (full_path, hg_file_node_id) in filenodes {
                checker.add_edge_with_path(
                    &mut edges,
                    EdgeType::HgManifestToHgFileEnvelope,
                    || Node::HgFileEnvelope(hg_file_node_id),
                    || Some(full_path.clone()),
                );
                checker.add_edge(&mut edges, EdgeType::HgManifestToHgFileNode, || {
                    Node::HgFileNode((full_path, hg_file_node_id))
                });
            }
            for (full_path, hg_child_manifest_id) in manifests {
                checker.add_edge(&mut edges, EdgeType::HgManifestToChildHgManifest, || {
                    Node::HgManifest((full_path, hg_child_manifest_id))
                })
            }
            StepOutput(
                checker.step_data(NodeType::HgManifest, || NodeData::HgManifest(hgmanifest)),
                edges,
            )
        })
}

fn alias_content_mapping_step<'a, V: VisitOne>(
    ctx: CoreContext,
    repo: &'a BlobRepo,
    checker: &'a Checker<V>,
    alias: Alias,
) -> impl Future<Output = Result<StepOutput, Error>> + 'a {
    alias
        .load(ctx, &repo.get_blobstore())
        .map_ok(move |content_id| {
            let mut edges = vec![];
            checker.add_edge(
                &mut edges,
                EdgeType::AliasContentMappingToFileContent,
                || Node::FileContent(content_id),
            );
            StepOutput(
                checker.step_data(NodeType::AliasContentMapping, || {
                    NodeData::AliasContentMapping(content_id)
                }),
                edges,
            )
        })
        .map_err(Error::from)
}

async fn bonsai_to_fsnode_mapping_step<V: VisitOne>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    bcs_id: &ChangesetId,
    enable_derive: bool,
) -> Result<StepOutput, Error> {
    let is_derived = RootFsnodeId::is_derived(&ctx, &repo, &bcs_id).await?;

    if is_derived || enable_derive {
        let mut edges = vec![];
        let root_fsnode_id = RootFsnodeId::derive(ctx.clone(), repo.clone(), *bcs_id)
            .map_err(Error::from)
            .compat()
            .await?;
        checker.add_edge(&mut edges, EdgeType::BonsaiToRootFsnode, || {
            Node::Fsnode((WrappedPath::Root, *root_fsnode_id.fsnode_id()))
        });
        Ok(StepOutput(
            checker.step_data(NodeType::BonsaiFsnodeMapping, || {
                NodeData::BonsaiFsnodeMapping(Some(*root_fsnode_id.fsnode_id()))
            }),
            edges,
        ))
    } else {
        Ok(StepOutput(
            checker.step_data(NodeType::BonsaiFsnodeMapping, || {
                NodeData::BonsaiFsnodeMapping(None)
            }),
            vec![],
        ))
    }
}

async fn fsnode_step<V: VisitOne>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker: &Checker<V>,
    path: WrappedPath,
    fsnode_id: &FsnodeId,
) -> Result<StepOutput, Error> {
    let fsnode = fsnode_id
        .load(ctx.clone(), &repo.get_blobstore())
        .map_err(Error::from)
        .await?;

    let mut edges = vec![];
    for (child, fsnode_entry) in fsnode.list() {
        // Fsnode do not have separate "file" entries, so we visit only directories
        match fsnode_entry {
            FsnodeEntry::Directory(dir) => {
                let fsnode_id = dir.id();
                let mpath_opt =
                    WrappedPath::from(MPath::join_element_opt(path.as_ref(), Some(child)));
                checker.add_edge(&mut edges, EdgeType::FsnodeToChildFsnode, || {
                    Node::Fsnode((WrappedPath::from(mpath_opt), *fsnode_id))
                });
            }
            FsnodeEntry::File(file) => {
                checker.add_edge_with_path(
                    &mut edges,
                    EdgeType::FsnodeToFileContent,
                    || Node::FileContent(*file.content_id()),
                    || {
                        let p =
                            WrappedPath::from(MPath::join_element_opt(path.as_ref(), Some(child)));
                        Some(p)
                    },
                );
            }
        }
    }

    Ok(StepOutput(
        checker.step_data(NodeType::Fsnode, || NodeData::Fsnode(fsnode)),
        edges,
    ))
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
            _ => (),
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
}

impl<V: VisitOne> Checker<V> {
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
    scuba: ScubaSampleBuilder,
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

    let checker = Arc::new(Checker {
        include_edge_types,
        always_emit_edge_types,
        keep_edge_paths,
        visitor: visitor.clone(),
        required_node_data_types,
    });

    published_bookmarks
        .map_ok(move |published_bookmarks| {
            let published_bookmarks = Arc::new(published_bookmarks);
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
                            published_bookmarks.clone(),
                            Arc::new(move |_ctx: &CoreContext| {
                                future::ok(
                                    published_bookmarks
                                        .iter()
                                        .map(|(_, csid)| csid)
                                        .cloned()
                                        .collect(),
                                )
                                .boxed()
                            }),
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
    mut scuba: ScubaSampleBuilder,
    published_bookmarks: Arc<HashMap<BookmarkName, ChangesetId>>,
    heads_fetcher: HeadsFetcher,
    checker: Arc<Checker<V>>,
) -> Result<(VOut, Vec<(Option<Route>, OutgoingEdge)>), Error>
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
        Node::Root => Err(format_err!("Not expecting Roots to be generated")),
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
        Node::BonsaiChangeset(bcs_id) => {
            bonsai_changeset_step(&ctx, &repo, &checker, &bcs_id).await
        }
        Node::BonsaiHgMapping(bcs_id) => {
            bonsai_to_hg_mapping_step(&ctx, &repo, &checker, bcs_id, enable_derive).await
        }
        Node::BonsaiPhaseMapping(bcs_id) => {
            let phases_store = repo.get_phases_factory().get_phases(
                repo.get_repoid(),
                repo.get_changeset_fetcher(),
                heads_fetcher.clone(),
            );
            bonsai_phase_step(&ctx, &checker, phases_store, bcs_id).await
        }
        Node::PublishedBookmarks => {
            published_bookmarks_step(published_bookmarks.clone(), &checker).await
        }
        // Hg
        Node::HgBonsaiMapping(hg_csid) => {
            hg_to_bonsai_mapping_step(ctx.clone(), &repo, &checker, hg_csid).await
        }
        Node::HgChangeset(hg_csid) => {
            hg_changeset_step(ctx.clone(), &repo, &checker, hg_csid).await
        }
        Node::HgFileEnvelope(hg_file_node_id) => {
            hg_file_envelope_step(
                ctx.clone(),
                &repo,
                &checker,
                hg_file_node_id,
                walk_item.path.as_ref(),
            )
            .await
        }
        Node::HgFileNode((path, hg_file_node_id)) => {
            hg_file_node_step(ctx.clone(), &repo, &checker, path, hg_file_node_id).await
        }
        Node::HgManifest((path, hg_manifest_id)) => {
            hg_manifest_step(ctx.clone(), &repo, &checker, path, hg_manifest_id).await
        }
        // Content
        Node::FileContent(content_id) => {
            file_content_step(ctx.clone(), &repo, &checker, content_id)
        }
        Node::FileContentMetadata(content_id) => {
            file_content_metadata_step(ctx.clone(), &repo, &checker, content_id, enable_derive)
                .await
        }
        Node::AliasContentMapping(alias) => {
            alias_content_mapping_step(ctx.clone(), &repo, &checker, alias).await
        }
        Node::BonsaiFsnodeMapping(cs_id) => {
            bonsai_to_fsnode_mapping_step(&ctx, &repo, &checker, &cs_id, enable_derive).await
        }
        Node::Fsnode((path, fsnode_id)) => {
            fsnode_step(&ctx, &repo, &checker, path, &fsnode_id).await
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
                .add(EDGE_TYPE, edge_label.to_str())
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
            let children = children
                .into_iter()
                .map(|c| {
                    if c.label.outgoing_type() != c.target.get_type() {
                        Err(format_err!(
                            "Bad step {:?} to {:?}",
                            c.label,
                            c.target.get_type()
                        ))
                    } else if c
                        .label
                        .incoming_type()
                        .map(|t| t != node_type)
                        .unwrap_or(false)
                    {
                        Err(format_err!("Bad step {:?} from {:?}", c.label, node_type,))
                    } else {
                        Ok(c)
                    }
                })
                .collect::<Result<Vec<OutgoingEdge>, Error>>();

            let children = children?;

            // Allow WalkVisitor to record state and decline outgoing nodes if already visited
            Ok(visitor.visit(&ctx, walk_item, Some(node_data), via, children)).map(
                |(vout, via, next)| {
                    let via = Some(via);
                    let next = next.into_iter().map(|e| (via.clone(), e)).collect();
                    (vout, next)
                },
            )
        }
    }
}
