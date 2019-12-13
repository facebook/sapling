/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::graph::{EdgeType, FileContentData, Node, NodeData, NodeType};
use anyhow::{format_err, Error};
use blobrepo::BlobRepo;
use blobstore::Loadable;
use bookmarks::BookmarkName;
use cloned::cloned;
use context::CoreContext;
use filestore::Alias;
use futures::{
    future::{self},
    Future,
};
use futures_ext::{
    bounded_traversal::bounded_traversal_stream, spawn_future, try_boxfuture, BoxFuture, BoxStream,
    FutureExt, StreamExt,
};
use itertools::{Either, Itertools};
use mercurial_types::{HgChangesetId, HgEntryId, HgFileNodeId, HgManifest, HgManifestId, RepoPath};
use mononoke_types::{ChangesetId, ContentId, MPath};
use std::{cmp, iter::IntoIterator, ops::Add};

// Holds type of edge and destination Node that we want to load in next step(s)
// Combined with current node, this forms an complegte edge.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OutgoingEdge {
    pub label: EdgeType,
    pub dest: Node,
}

impl OutgoingEdge {
    pub fn new(label: EdgeType, dest: Node) -> Self {
        Self { label, dest }
    }
}

pub trait WalkVisitor {
    // This can mutate the internal state.  Returns true if we should visit the outgoing_edge
    fn record_outgoing(
        &self,
        current: Option<(&Node, &NodeData)>,
        outgoing_edge: &OutgoingEdge,
    ) -> bool;

    // How many times has the vistor seen this type
    fn get_visit_count(&self, t: &NodeType) -> usize;
}

// Data found for this node, plus next steps
struct StepOutput(NodeData, Vec<OutgoingEdge>);

fn bookmark_step(
    ctx: CoreContext,
    repo: &BlobRepo,
    b: BookmarkName,
) -> BoxFuture<StepOutput, Error> {
    repo.get_bonsai_bookmark(ctx, &b)
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
                Ok(StepOutput(NodeData::Bookmark(bcs_id), recurse))
            }
            None => Err(format_err!("Unknown Bookmark {}", b)),
        })
        .boxify()
}

fn bonsai_changeset_step(
    ctx: CoreContext,
    repo: &BlobRepo,
    bcs_id: ChangesetId,
) -> BoxFuture<StepOutput, Error> {
    // Get the data, and add direct file data for this bonsai changeset
    repo.get_bonsai_changeset(ctx, bcs_id)
        .map(move |bcs| {
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
                Node::BonsaiHgMapping(bcs_id),
            ));
            recurse.append(
                &mut bcs
                    .file_changes()
                    .filter_map(|(_mpath, fc_opt)| {
                        fc_opt // remove None
                    })
                    .map(|fc| {
                        OutgoingEdge::new(
                            EdgeType::BonsaiChangesetToFileContent,
                            Node::FileContent(fc.content_id()),
                        )
                    })
                    .collect::<Vec<OutgoingEdge>>(),
            );
            StepOutput(NodeData::BonsaiChangeset(bcs), recurse)
        })
        .boxify()
}

fn file_content_step(
    ctx: CoreContext,
    repo: &BlobRepo,
    id: ContentId,
) -> BoxFuture<StepOutput, Error> {
    let s = repo.get_file_content_by_content_id(ctx, id);
    // We don't force file loading here, content may not be needed
    future::ok(StepOutput(
        NodeData::FileContent(FileContentData::ContentStream(s)),
        vec![],
    ))
    .boxify()
}

fn file_content_metadata_step(
    ctx: CoreContext,
    repo: &BlobRepo,
    id: ContentId,
) -> BoxFuture<StepOutput, Error> {
    repo.get_file_content_metadata(ctx, id)
        .map(|metadata| {
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
                    Node::AliasContentMapping(Alias::GitSha1(metadata.git_sha1)),
                ),
            ];
            StepOutput(NodeData::FileContentMetadata(metadata), recurse)
        })
        .boxify()
}

fn bonsai_to_hg_mapping_step(
    ctx: CoreContext,
    repo: &BlobRepo,
    bcs_id: ChangesetId,
) -> BoxFuture<StepOutput, Error> {
    repo.get_hg_from_bonsai_changeset(ctx, bcs_id)
        .map({
            |hg_cs_id| {
                StepOutput(
                    NodeData::BonsaiHgMapping(hg_cs_id),
                    vec![OutgoingEdge::new(
                        EdgeType::BonsaiHgMappingToHgChangeset,
                        Node::HgChangeset(hg_cs_id),
                    )],
                )
            }
        })
        .boxify()
}

fn hg_to_bonsai_mapping_step(
    ctx: CoreContext,
    repo: &BlobRepo,
    id: HgChangesetId,
) -> BoxFuture<StepOutput, Error> {
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
        .boxify()
}

fn hg_changeset_step(
    ctx: CoreContext,
    repo: &BlobRepo,
    id: HgChangesetId,
) -> BoxFuture<StepOutput, Error> {
    repo.get_changeset_by_changesetid(ctx, id)
        .map(|hgchangeset| {
            let manifest_id = hgchangeset.manifestid();
            let mut recurse = vec![OutgoingEdge::new(
                EdgeType::HgChangesetToHgManifest,
                Node::HgManifest((None, manifest_id)),
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
        .boxify()
}

fn hg_file_envelope_step(
    ctx: CoreContext,
    repo: &BlobRepo,
    hg_file_node_id: HgFileNodeId,
) -> BoxFuture<StepOutput, Error>
where {
    repo.get_file_envelope(ctx, hg_file_node_id)
        .map({
            move |envelope| {
                let file_content_id = envelope.content_id();
                let fnode = OutgoingEdge::new(
                    EdgeType::HgFileEnvelopeToFileContent,
                    Node::FileContent(file_content_id),
                );
                StepOutput(NodeData::HgFileEnvelope(envelope), vec![fnode])
            }
        })
        .boxify()
}

fn hg_file_node_step(
    ctx: CoreContext,
    repo: &BlobRepo,
    path: Option<MPath>,
    hg_file_node_id: HgFileNodeId,
) -> BoxFuture<StepOutput, Error> {
    let repo_path = match path.clone() {
        None => RepoPath::RootPath,
        Some(mpath) => RepoPath::FilePath(mpath),
    };
    repo.get_filenode_opt(ctx, &repo_path, hg_file_node_id)
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
                file_node_info
                    .clone()
                    .copyfrom
                    .map(|(repo_path, file_node_id)| {
                        recurse.push(OutgoingEdge::new(
                            EdgeType::HgFileNodeToHgCopyfromFileNode,
                            Node::HgFileNode((repo_path.into_mpath(), file_node_id)),
                        ))
                    });
                StepOutput(NodeData::HgFileNode(Some(file_node_info)), recurse)
            }
            None => StepOutput(NodeData::HgFileNode(None), vec![]),
        })
        .boxify()
}

fn hg_manifest_step(
    ctx: CoreContext,
    repo: &BlobRepo,
    path: Option<MPath>,
    hg_manifest_id: HgManifestId,
) -> BoxFuture<StepOutput, Error> {
    repo.get_manifest_by_nodeid(ctx, hg_manifest_id)
        .map({
            move |hgmanifest| {
                let (manifests, filenodes): (Vec<_>, Vec<_>) =
                    hgmanifest.list().partition_map(|child| {
                        let mpath_opt = MPath::join_element_opt(path.as_ref(), child.get_name());
                        match child.get_hash() {
                            HgEntryId::File(_, filenode_id) => {
                                Either::Right((mpath_opt, filenode_id))
                            }
                            HgEntryId::Manifest(manifest_id) => {
                                Either::Left((mpath_opt, manifest_id))
                            }
                        }
                    });

                let mut children: Vec<_> = filenodes
                    .into_iter()
                    .map(move |(full_path, hg_file_node_id)| {
                        vec![
                            OutgoingEdge::new(
                                EdgeType::HgManifestToHgFileEnvelope,
                                Node::HgFileEnvelope(hg_file_node_id),
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
        .boxify()
}

fn alias_content_mapping_step(
    ctx: CoreContext,
    repo: &BlobRepo,
    alias: Alias,
) -> BoxFuture<StepOutput, Error> {
    alias
        .load(ctx, &repo.get_blobstore())
        .map(|content_id| {
            let recurse = vec![OutgoingEdge::new(
                EdgeType::AliasContentMappingToFileContent,
                Node::FileContent(content_id),
            )];
            StepOutput(NodeData::AliasContentMapping(content_id), recurse)
        })
        .map_err(Error::from)
        .boxify()
}

/// Expand nodes where check for a type is used as a check for other types.
/// e.g. to make sure metadata looked up/considered for files.
fn expand_checked_nodes(children: &mut Vec<OutgoingEdge>) -> () {
    let mut extra = vec![];
    for n in children.iter() {
        match n {
            OutgoingEdge {
                label: _,
                dest: Node::FileContent(fc_id),
            } => {
                extra.push(OutgoingEdge::new(
                    EdgeType::FileContentToFileContentMetadata,
                    Node::FileContentMetadata(*fc_id),
                ));
            }
            _ => (),
        }
    }
    if !extra.is_empty() {
        children.append(&mut extra);
    }
}

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct StepStats {
    pub num_direct: usize,
    pub num_direct_new: usize,
    pub num_expanded_new: usize,
    pub visited_of_type: usize,
}

impl Add<StepStats> for StepStats {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self {
            num_direct: self.num_direct + other.num_direct,
            num_direct_new: self.num_direct_new + other.num_direct_new,
            num_expanded_new: self.num_expanded_new + other.num_expanded_new,
            visited_of_type: cmp::max(self.visited_of_type, other.visited_of_type),
        }
    }
}

/// Walk the graph from one or more starting points,  providing stream of data for later reduction
pub fn walk_exact<FilterItem, V>(
    ctx: CoreContext,
    repo: BlobRepo,
    walk_roots: Vec<OutgoingEdge>,
    visitor: V,
    filter_item: FilterItem,
    scheduled_max: usize,
) -> BoxStream<(Node, Option<(StepStats, NodeData)>), Error>
where
    FilterItem: 'static + Send + Clone + Fn(&Node, &NodeData, &OutgoingEdge) -> bool,
    V: 'static + Clone + WalkVisitor + Send,
{
    // record the roots so the stats add up
    walk_roots.iter().for_each(|e| {
        visitor.record_outgoing(None, e);
    });

    bounded_traversal_stream(scheduled_max, walk_roots, {
        // Each step returns the walk result, and next steps
        move |walk_item| {
            cloned!(ctx);
            let node = walk_item.dest.clone();
            let next = match node.clone() {
                // Bonsai
                Node::Bookmark(bookmark_name) => bookmark_step(ctx, &repo, bookmark_name),
                Node::BonsaiChangeset(bcs_id) => bonsai_changeset_step(ctx, &repo, bcs_id),
                Node::BonsaiHgMapping(bcs_id) => bonsai_to_hg_mapping_step(ctx, &repo, bcs_id),
                // Hg
                Node::HgBonsaiMapping(hg_csid) => hg_to_bonsai_mapping_step(ctx, &repo, hg_csid),
                Node::HgChangeset(hg_csid) => hg_changeset_step(ctx, &repo, hg_csid),
                Node::HgFileEnvelope(hg_file_node_id) => {
                    hg_file_envelope_step(ctx, &repo, hg_file_node_id)
                }
                Node::HgFileNode((path, hg_file_node_id)) => {
                    hg_file_node_step(ctx, &repo, path, hg_file_node_id)
                }
                Node::HgManifest((path, hg_manifest_id)) => {
                    hg_manifest_step(ctx, &repo, path, hg_manifest_id)
                }
                // Content
                Node::FileContent(content_id) => file_content_step(ctx, &repo, content_id),
                Node::FileContentMetadata(content_id) => {
                    file_content_metadata_step(ctx, &repo, content_id)
                }
                Node::AliasContentMapping(alias) => alias_content_mapping_step(ctx, &repo, alias),
            }
            .and_then({
                cloned!(filter_item, visitor);
                move |StepOutput(node_data, children)| {
                    // make sure steps are valid.  would be nice if this could be static
                    let children = children
                        .into_iter()
                        .map(|c| {
                            if c.label.outgoing_type() != c.dest.get_type() {
                                Err(format_err!(
                                    "Bad step {:?} to {:?}",
                                    c.label,
                                    c.dest.get_type()
                                ))
                            } else if c
                                .label
                                .incoming_type()
                                .map(|t| t != node.get_type())
                                .unwrap_or(false)
                            {
                                Err(format_err!(
                                    "Bad step {:?} from {:?}",
                                    c.label,
                                    node.get_type()
                                ))
                            } else {
                                Ok(c)
                            }
                        })
                        .collect::<Result<Vec<OutgoingEdge>, Error>>();

                    let mut children = try_boxfuture!(children);

                    // Filter things we don't want to enter the WalkVisitor at all.
                    children.retain(|outgoing_edge| {
                        filter_item.clone()(&node, &node_data, outgoing_edge)
                    });
                    let num_direct = children.len();

                    // Allow WalkVisitor to record state and decline outgoing nodes if already visited
                    children.retain(|outgoing_edge| {
                        visitor.record_outgoing(Some((&node, &node_data)), outgoing_edge)
                    });
                    let num_direct_new = children.len();

                    expand_checked_nodes(&mut children);
                    // Make sure we don't add in types of node and edge not wanted
                    children.retain(|outgoing_edge| {
                        filter_item.clone()(&node, &node_data, outgoing_edge)
                    });
                    let num_expanded_new = children.len();

                    let visited_of_type = visitor.get_visit_count(&node.get_type());

                    let stats = StepStats {
                        num_direct,
                        num_direct_new,
                        num_expanded_new,
                        visited_of_type,
                    };
                    future::ok(((node, Some((stats, node_data))), children)).boxify()
                }
            });
            spawn_future(next)
        }
    })
    .boxify()
}
