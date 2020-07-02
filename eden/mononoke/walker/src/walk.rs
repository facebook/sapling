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
    iter::{IntoIterator, Iterator},
    sync::Arc,
};
use thiserror::Error;

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
    #[error("Could not step to {0:?}")]
    NotTraversable(OutgoingEdge),
}

pub trait WalkVisitor<VOut, Route> {
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

fn bookmark_step(
    ctx: CoreContext,
    repo: BlobRepo,
    b: BookmarkName,
    published_bookmarks: Arc<HashMap<BookmarkName, ChangesetId>>,
) -> impl Future<Output = Result<StepOutput, Error>> {
    match published_bookmarks.get(&b) {
        Some(csid) => future::ok(Some(csid.clone())).left_future(),
        // Just in case we have non-public bookmarks
        None => repo.get_bonsai_bookmark(ctx, &b).compat().right_future(),
    }
    .and_then(move |bcs_opt| match bcs_opt {
        Some(bcs_id) => {
            let recurse = vec![
                OutgoingEdge::new(
                    EdgeType::BookmarkToBonsaiChangeset,
                    Node::BonsaiChangeset(bcs_id),
                ),
                OutgoingEdge::new(
                    EdgeType::BookmarkToBonsaiHgMapping,
                    Node::BonsaiHgMapping(bcs_id),
                ),
            ];
            future::ok(StepOutput(NodeData::Bookmark(bcs_id), recurse))
        }
        None => future::err(format_err!("Unknown Bookmark {}", b)),
    })
}

fn published_bookmarks_step(
    published_bookmarks: Arc<HashMap<BookmarkName, ChangesetId>>,
) -> impl Future<Output = Result<StepOutput, Error>> {
    let mut recurse = vec![];
    for (_, bcs_id) in published_bookmarks.iter() {
        recurse.push(OutgoingEdge::new(
            EdgeType::PublishedBookmarksToBonsaiChangeset,
            Node::BonsaiChangeset(bcs_id.clone()),
        ));
        recurse.push(OutgoingEdge::new(
            EdgeType::PublishedBookmarksToBonsaiHgMapping,
            Node::BonsaiHgMapping(bcs_id.clone()),
        ));
    }
    future::ok(StepOutput(NodeData::PublishedBookmarks, recurse))
}

fn bonsai_phase_step(
    ctx: &CoreContext,
    phases_store: Arc<dyn Phases>,
    bcs_id: ChangesetId,
) -> impl Future<Output = Result<StepOutput, Error>> {
    phases_store
        .get_public(ctx.clone(), vec![bcs_id], true)
        .map(move |public| public.contains(&bcs_id))
        .map(|is_public| {
            let phase = if is_public { Some(Phase::Public) } else { None };
            StepOutput(NodeData::BonsaiPhaseMapping(phase), vec![])
        })
        .compat()
}

async fn bonsai_changeset_step(
    ctx: &CoreContext,
    repo: &BlobRepo,
    bcs_id: &ChangesetId,
    keep_edge_paths: bool,
) -> Result<StepOutput, Error> {
    // Get the data, and add direct file data for this bonsai changeset
    let bcs = bcs_id.load(ctx.clone(), repo.blobstore()).await?;

    // Parents deliberately first to resolve dependent reads as early as possible
    let mut recurse: Vec<OutgoingEdge> = bcs
        .parents()
        .map(|parent_id| {
            OutgoingEdge::new(
                EdgeType::BonsaiChangesetToBonsaiParent,
                Node::BonsaiChangeset(parent_id),
            )
        })
        .collect();
    // Allow Hg based lookup
    recurse.push(OutgoingEdge::new(
        EdgeType::BonsaiChangesetToBonsaiHgMapping,
        Node::BonsaiHgMapping(*bcs_id),
    ));
    // Lookup phases
    recurse.push(OutgoingEdge::new(
        EdgeType::BonsaiChangesetToBonsaiPhaseMapping,
        Node::BonsaiPhaseMapping(*bcs_id),
    ));
    recurse.append(
        &mut bcs
            .file_changes()
            .filter_map(|(mpath, fc_opt)| {
                fc_opt.map(|fc| (fc, mpath)) // remove None
            })
            .map(|(fc, mpath)| {
                OutgoingEdge::new_with_path(
                    EdgeType::BonsaiChangesetToFileContent,
                    Node::FileContent(fc.content_id()),
                    if keep_edge_paths {
                        Some(WrappedPath::from(Some(mpath.clone())))
                    } else {
                        None
                    },
                )
            })
            .collect::<Vec<OutgoingEdge>>(),
    );
    recurse.push(OutgoingEdge::new(
        EdgeType::BonsaiChangesetToBonsaiFsnodeMapping,
        Node::BonsaiFsnodeMapping(*bcs_id),
    ));
    Ok(StepOutput(NodeData::BonsaiChangeset(bcs), recurse))
}

fn file_content_step(
    ctx: CoreContext,
    repo: &BlobRepo,
    id: ContentId,
) -> Result<StepOutput, Error> {
    let s = filestore::fetch_stream(repo.blobstore(), ctx, id)
        .map(FileBytes)
        .compat();
    // We don't force file loading here, content may not be needed
    Ok(StepOutput(
        NodeData::FileContent(FileContentData::ContentStream(Box::pin(s))),
        vec![],
    ))
}

fn file_content_metadata_step(
    ctx: CoreContext,
    repo: &BlobRepo,
    id: ContentId,
    enable_derive: bool,
) -> impl Future<Output = Result<StepOutput, Error>> {
    let loader = if enable_derive {
        filestore::get_metadata(repo.blobstore(), ctx, &id.into())
            .map(Some)
            .left_future()
    } else {
        filestore::get_metadata_readonly(repo.blobstore(), ctx, &id.into()).right_future()
    };

    loader
        .map(|metadata_opt| match metadata_opt {
            Some(Some(metadata)) => {
                let recurse = vec![
                    OutgoingEdge::new(
                        EdgeType::FileContentMetadataToSha1Alias,
                        Node::AliasContentMapping(Alias::Sha1(metadata.sha1)),
                    ),
                    OutgoingEdge::new(
                        EdgeType::FileContentMetadataToSha256Alias,
                        Node::AliasContentMapping(Alias::Sha256(metadata.sha256)),
                    ),
                    OutgoingEdge::new(
                        EdgeType::FileContentMetadataToGitSha1Alias,
                        Node::AliasContentMapping(Alias::GitSha1(metadata.git_sha1.sha1())),
                    ),
                ];
                StepOutput(NodeData::FileContentMetadata(Some(metadata)), recurse)
            }
            Some(None) | None => StepOutput(NodeData::FileContentMetadata(None), vec![]),
        })
        .compat()
}

fn bonsai_to_hg_mapping_step<'a>(
    ctx: &'a CoreContext,
    repo: &'a BlobRepo,
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
        .map(|maybe_hg_cs_id| match maybe_hg_cs_id {
            Some(hg_cs_id) => StepOutput(
                NodeData::BonsaiHgMapping(Some(hg_cs_id)),
                vec![OutgoingEdge::new(
                    EdgeType::BonsaiHgMappingToHgChangeset,
                    Node::HgChangeset(hg_cs_id),
                )],
            ),
            None => StepOutput(NodeData::BonsaiHgMapping(None), vec![]),
        })
        .compat()
}

fn hg_to_bonsai_mapping_step(
    ctx: CoreContext,
    repo: &BlobRepo,
    id: HgChangesetId,
) -> impl Future<Output = Result<StepOutput, Error>> {
    repo.get_bonsai_from_hg(ctx, id)
        .map(move |maybe_bcs_id| match maybe_bcs_id {
            Some(bcs_id) => {
                let recurse = vec![OutgoingEdge::new(
                    EdgeType::HgBonsaiMappingToBonsaiChangeset,
                    Node::BonsaiChangeset(bcs_id),
                )];
                StepOutput(NodeData::HgBonsaiMapping(Some(bcs_id)), recurse)
            }
            None => StepOutput(NodeData::HgBonsaiMapping(None), vec![]),
        })
        .compat()
}

fn hg_changeset_step(
    ctx: CoreContext,
    repo: &BlobRepo,
    id: HgChangesetId,
) -> impl Future<Output = Result<StepOutput, Error>> {
    id.load(ctx, repo.blobstore())
        .map_err(Error::from)
        .map_ok(|hgchangeset| {
            let manifest_id = hgchangeset.manifestid();
            let mut recurse = vec![OutgoingEdge::new(
                EdgeType::HgChangesetToHgManifest,
                Node::HgManifest((WrappedPath::Root, manifest_id)),
            )];
            for p in hgchangeset.parents().into_iter() {
                let step = OutgoingEdge::new(
                    EdgeType::HgChangesetToHgParent,
                    Node::HgChangeset(HgChangesetId::new(p)),
                );
                recurse.push(step);
            }
            StepOutput(NodeData::HgChangeset(hgchangeset), recurse)
        })
}

fn hg_file_envelope_step(
    ctx: CoreContext,
    repo: &BlobRepo,
    hg_file_node_id: HgFileNodeId,
    path: Option<WrappedPath>,
) -> impl Future<Output = Result<StepOutput, Error>> {
    hg_file_node_id
        .load(ctx, repo.blobstore())
        .map_err(Error::from)
        .map_ok({
            move |envelope| {
                let file_content_id = envelope.content_id();
                let fnode = OutgoingEdge::new_with_path(
                    EdgeType::HgFileEnvelopeToFileContent,
                    Node::FileContent(file_content_id),
                    path,
                );
                StepOutput(NodeData::HgFileEnvelope(envelope), vec![fnode])
            }
        })
}

fn hg_file_node_step(
    ctx: CoreContext,
    repo: &BlobRepo,
    path: WrappedPath,
    hg_file_node_id: HgFileNodeId,
) -> impl Future<Output = Result<StepOutput, Error>> {
    let repo_path = match &path {
        WrappedPath::Root => RepoPath::RootPath,
        WrappedPath::NonRoot(path) => RepoPath::FilePath(path.mpath().clone()),
    };
    repo.get_filenode_opt(ctx, &repo_path, hg_file_node_id)
        .and_then(|filenode| filenode.do_not_handle_disabled_filenodes())
        .map(move |file_node_opt| match file_node_opt {
            Some(file_node_info) => {
                // Validate hg link node
                let linked_commit = OutgoingEdge::new(
                    EdgeType::HgLinkNodeToHgChangeset,
                    Node::HgChangeset(file_node_info.linknode),
                );
                // Following linknode bonsai increases parallelism of walk.
                // Linknodes will point to many commits we can then walk
                // in parallel
                let linked_commit_bonsai = OutgoingEdge::new(
                    EdgeType::HgLinkNodeToHgBonsaiMapping,
                    Node::HgBonsaiMapping(file_node_info.linknode),
                );
                let mut recurse = vec![linked_commit, linked_commit_bonsai];
                file_node_info.p1.map(|parent_file_node_id| {
                    recurse.push(OutgoingEdge::new(
                        EdgeType::HgFileNodeToHgParentFileNode,
                        Node::HgFileNode((path.clone(), parent_file_node_id)),
                    ))
                });
                file_node_info.p2.map(|parent_file_node_id| {
                    recurse.push(OutgoingEdge::new(
                        EdgeType::HgFileNodeToHgParentFileNode,
                        Node::HgFileNode((path.clone(), parent_file_node_id)),
                    ))
                });
                // Copyfrom is like another parent
                for (repo_path, file_node_id) in &file_node_info.copyfrom {
                    recurse.push(OutgoingEdge::new(
                        EdgeType::HgFileNodeToHgCopyfromFileNode,
                        Node::HgFileNode((
                            WrappedPath::from(repo_path.clone().into_mpath()),
                            *file_node_id,
                        )),
                    ))
                }
                StepOutput(NodeData::HgFileNode(Some(file_node_info)), recurse)
            }
            None => StepOutput(NodeData::HgFileNode(None), vec![]),
        })
        .compat()
}

fn hg_manifest_step(
    ctx: CoreContext,
    repo: &BlobRepo,
    path: WrappedPath,
    hg_manifest_id: HgManifestId,
    keep_edge_paths: bool,
) -> impl Future<Output = Result<StepOutput, Error>> {
    hg_manifest_id
        .load(ctx, repo.blobstore())
        .map_err(Error::from)
        .map_ok({
            move |hgmanifest| {
                let (manifests, filenodes): (Vec<_>, Vec<_>) =
                    hgmanifest.list().partition_map(|child| {
                        let path_opt = WrappedPath::from(MPath::join_element_opt(
                            path.as_ref(),
                            child.get_name(),
                        ));
                        match child.get_hash() {
                            HgEntryId::File(_, filenode_id) => {
                                Either::Right((path_opt, filenode_id))
                            }
                            HgEntryId::Manifest(manifest_id) => {
                                Either::Left((path_opt, manifest_id))
                            }
                        }
                    });

                let mut children: Vec<_> = filenodes
                    .into_iter()
                    .map(move |(full_path, hg_file_node_id)| {
                        vec![
                            OutgoingEdge::new_with_path(
                                EdgeType::HgManifestToHgFileEnvelope,
                                Node::HgFileEnvelope(hg_file_node_id),
                                if keep_edge_paths {
                                    Some(full_path.clone())
                                } else {
                                    None
                                },
                            ),
                            OutgoingEdge::new(
                                EdgeType::HgManifestToHgFileNode,
                                Node::HgFileNode((full_path, hg_file_node_id)),
                            ),
                        ]
                    })
                    .flatten()
                    .collect();

                let mut children_manifests: Vec<_> = manifests
                    .into_iter()
                    .map(move |(full_path, hg_child_manifest_id)| {
                        OutgoingEdge::new(
                            EdgeType::HgManifestToChildHgManifest,
                            Node::HgManifest((full_path, hg_child_manifest_id)),
                        )
                    })
                    .collect();

                children.append(&mut children_manifests);

                StepOutput(NodeData::HgManifest(hgmanifest), children)
            }
        })
}

fn alias_content_mapping_step(
    ctx: CoreContext,
    repo: &BlobRepo,
    alias: Alias,
) -> impl Future<Output = Result<StepOutput, Error>> {
    alias
        .load(ctx, &repo.get_blobstore())
        .map_ok(|content_id| {
            let recurse = vec![OutgoingEdge::new(
                EdgeType::AliasContentMappingToFileContent,
                Node::FileContent(content_id),
            )];
            StepOutput(NodeData::AliasContentMapping(content_id), recurse)
        })
        .map_err(Error::from)
}

async fn bonsai_to_fsnode_mapping_step(
    ctx: &CoreContext,
    repo: &BlobRepo,
    bcs_id: &ChangesetId,
    enable_derive: bool,
) -> Result<StepOutput, Error> {
    let is_derived = RootFsnodeId::is_derived(&ctx, &repo, &bcs_id).await?;

    if is_derived || enable_derive {
        let root_fsnode_id = RootFsnodeId::derive(ctx.clone(), repo.clone(), *bcs_id)
            .map_err(Error::from)
            .compat()
            .await?;

        Ok(StepOutput(
            NodeData::BonsaiFsnodeMapping(Some(*root_fsnode_id.fsnode_id())),
            vec![OutgoingEdge::new(
                EdgeType::BonsaiToRootFsnode,
                Node::Fsnode((WrappedPath::Root, *root_fsnode_id.fsnode_id())),
            )],
        ))
    } else {
        Ok(StepOutput(NodeData::BonsaiFsnodeMapping(None), vec![]))
    }
}

async fn fsnode_step(
    ctx: &CoreContext,
    repo: &BlobRepo,
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
        if let FsnodeEntry::Directory(dir) = fsnode_entry {
            let fsnode_id = dir.id();
            let mpath_opt = WrappedPath::from(MPath::join_element_opt(path.as_ref(), Some(child)));

            edges.push(OutgoingEdge::new(
                EdgeType::FsnodeToChildFsnode,
                Node::Fsnode((WrappedPath::from(mpath_opt), *fsnode_id)),
            ));
        }
    }

    Ok(StepOutput(NodeData::Fsnode(fsnode), edges))
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
    scuba: ScubaSampleBuilder,
    keep_edge_paths: bool,
) -> BoxStream<'static, Result<VOut, Error>>
where
    V: 'static + Clone + WalkVisitor<VOut, Route> + Send,
    VOut: 'static + Send,
    Route: 'static + Send + Clone,
{
    // Build lookups
    let repoid = *(&repo.get_repoid());
    let published_bookmarks = repo
        .bookmarks()
        .list(
            ctx.clone(),
            repoid,
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
            bounded_traversal_stream(scheduled_max, walk_roots, {
                move |(via, walk_item)| {
                    let ctx = visitor.start_step(ctx.clone(), via.as_ref(), &walk_item);
                    cloned!(
                        error_as_data_node_types,
                        error_as_data_edge_types,
                        published_bookmarks,
                        repo,
                        scuba,
                        visitor
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
                            keep_edge_paths,
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
    keep_edge_paths: bool,
) -> Result<(VOut, Vec<(Option<Route>, OutgoingEdge)>), Error>
where
    V: 'static + Clone + WalkVisitor<VOut, Route> + Send,
    VOut: 'static + Send,
    Route: 'static + Send + Clone,
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
                repo.clone(),
                bookmark_name,
                published_bookmarks.clone(),
            )
            .await
        }
        Node::BonsaiChangeset(bcs_id) => {
            bonsai_changeset_step(&ctx, &repo, &bcs_id, keep_edge_paths).await
        }
        Node::BonsaiHgMapping(bcs_id) => {
            bonsai_to_hg_mapping_step(&ctx, &repo, bcs_id, enable_derive).await
        }
        Node::BonsaiPhaseMapping(bcs_id) => {
            let phases_store = repo.get_phases_factory().get_phases(
                repo.get_repoid(),
                repo.get_changeset_fetcher(),
                heads_fetcher.clone(),
            );
            bonsai_phase_step(&ctx, phases_store, bcs_id).await
        }
        Node::PublishedBookmarks => published_bookmarks_step(published_bookmarks.clone()).await,
        // Hg
        Node::HgBonsaiMapping(hg_csid) => {
            hg_to_bonsai_mapping_step(ctx.clone(), &repo, hg_csid).await
        }
        Node::HgChangeset(hg_csid) => hg_changeset_step(ctx.clone(), &repo, hg_csid).await,
        Node::HgFileEnvelope(hg_file_node_id) => {
            hg_file_envelope_step(
                ctx.clone(),
                &repo,
                hg_file_node_id,
                if keep_edge_paths {
                    walk_item.path.clone()
                } else {
                    None
                },
            )
            .await
        }
        Node::HgFileNode((path, hg_file_node_id)) => {
            hg_file_node_step(ctx.clone(), &repo, path, hg_file_node_id).await
        }
        Node::HgManifest((path, hg_manifest_id)) => {
            hg_manifest_step(ctx.clone(), &repo, path, hg_manifest_id, keep_edge_paths).await
        }
        // Content
        Node::FileContent(content_id) => file_content_step(ctx.clone(), &repo, content_id),
        Node::FileContentMetadata(content_id) => {
            file_content_metadata_step(ctx.clone(), &repo, content_id, enable_derive).await
        }
        Node::AliasContentMapping(alias) => {
            alias_content_mapping_step(ctx.clone(), &repo, alias).await
        }
        Node::BonsaiFsnodeMapping(cs_id) => {
            bonsai_to_fsnode_mapping_step(&ctx, &repo, &cs_id, enable_derive).await
        }
        Node::Fsnode((path, fsnode_id)) => fsnode_step(&ctx, &repo, path, &fsnode_id).await,
    };

    let edge_label = walk_item.label;
    let node_type = walk_item.target.get_type();
    let step_output = match step_result {
        Ok(s) => Ok(s),
        Err(e) => {
            // Log to scuba regardless
            add_node_to_scuba(None, &walk_item.target, &mut scuba);
            scuba
                .add(EDGE_TYPE, edge_label.to_str())
                .add(CHECK_TYPE, "step")
                .add(CHECK_FAIL, 1)
                .log();
            // Optionally attempt to continue
            if error_as_data_node_types.contains(&walk_item.target.get_type()) {
                if error_as_data_edge_types.is_empty()
                    || error_as_data_edge_types.contains(&walk_item.label)
                {
                    warn!(
                        logger,
                        "Could not step to {:?}, due to: {:?}", &walk_item, e
                    );
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
    .with_context(|| ErrorKind::NotTraversable(walk_item.clone()))?;

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
