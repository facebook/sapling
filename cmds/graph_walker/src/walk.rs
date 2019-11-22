/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::graph::{FileContentData, Node, NodeData, NodeType};
use blobrepo::BlobRepo;
use bookmarks::BookmarkName;
use changeset_fetcher::ChangesetFetcher;
use cloned::cloned;
use context::CoreContext;
use failure_ext::{format_err, Error};
use futures::{
    future::{self},
    Future,
};
use futures_ext::{
    bounded_traversal::bounded_traversal_stream, spawn_future, BoxFuture, BoxStream, FutureExt,
    StreamExt,
};
use itertools::{Either, Itertools};
use mercurial_types::{
    Changeset, HgChangesetId, HgEntryId, HgFileNodeId, HgManifest, HgManifestId, RepoPath,
};
use mononoke_types::{ChangesetId, ContentId, MPath};
use std::{cmp, iter::IntoIterator, ops::Add, sync::Arc};

pub trait NodeChecker {
    // This can mutate the internal state.  Returns true if we should visit the node
    fn record_visit(&self, n: &Node) -> bool;

    // How many times has the checker seen this type
    fn get_visit_count(&self, t: &NodeType) -> usize;
}

// This exists temporarily, until a step is put into the stream for next iteration
struct WalkStep(NodeData, Vec<Node>);

fn bookmark_step(ctx: CoreContext, repo: &BlobRepo, b: BookmarkName) -> BoxFuture<WalkStep, Error> {
    repo.get_bonsai_bookmark(ctx, &b)
        .and_then(move |bcs_opt| match bcs_opt {
            Some(bcs_id) => {
                let recurse = vec![Node::BonsaiChangeset(bcs_id)];
                Ok(WalkStep(NodeData::Bookmark(bcs_id), recurse))
            }
            None => Err(format_err!("Unknown bookmark {}", b)),
        })
        .boxify()
}

fn bonsai_changeset_step(
    ctx: CoreContext,
    repo: &BlobRepo,
    bcs_id: ChangesetId,
) -> BoxFuture<WalkStep, Error> {
    // Get the data, and add direct file data for this bonsai changeset
    let bonsai_fut = repo
        .get_bonsai_changeset(ctx, bcs_id)
        .map({
            move |bcs| {
                let files_to_visit: Vec<Node> = bcs
                    .file_changes()
                    .filter_map(|(_mpath, fc_opt)| {
                        fc_opt // remove None
                    })
                    .map(|fc| Node::FileContent(fc.content_id()))
                    .collect();
                (bcs, files_to_visit)
            }
        })
        .boxify();

    bonsai_fut
        .map(move |(bcs, mut children)| {
            // Parents deliberately first to resolve dependent reads as early as possible
            children.push(Node::BonsaiParents(bcs_id));
            // Allow Hg based lookup
            children.push(Node::HgChangesetFromBonsaiChangeset(bcs_id));
            WalkStep(NodeData::BonsaiChangeset(bcs), children)
        })
        .boxify()
}

fn bonsai_parents_step(
    ctx: CoreContext,
    changeset_fetcher: Arc<dyn ChangesetFetcher>,
    bcs_id: ChangesetId,
) -> BoxFuture<WalkStep, Error> {
    changeset_fetcher
        .get_parents(ctx, bcs_id)
        .map({
            move |parents| {
                let parents_to_visit: Vec<_> = {
                    parents
                        .iter()
                        .cloned()
                        .map(|p| Node::BonsaiChangeset(p))
                        .collect()
                };
                WalkStep(NodeData::BonsaiParents(parents), parents_to_visit)
            }
        })
        .boxify()
}

fn file_content_step(
    ctx: CoreContext,
    repo: &BlobRepo,
    id: ContentId,
) -> BoxFuture<WalkStep, Error> {
    let s = repo.get_file_content_by_content_id(ctx, id);
    // We don't force file loading here, content may not be needed
    future::ok(WalkStep(
        NodeData::FileContent(FileContentData::ContentStream(s)),
        vec![],
    ))
    .boxify()
}

fn file_content_metadata_step(
    ctx: CoreContext,
    repo: &BlobRepo,
    id: ContentId,
) -> BoxFuture<WalkStep, Error> {
    repo.get_file_content_metadata(ctx, id)
        .map(|metadata| {
            let recurse = vec![];
            // Could potentially recurse on aliases here.
            WalkStep(NodeData::FileContentMetadata(metadata), recurse)
        })
        .boxify()
}

fn hg_changeset_from_bonsai_step(
    ctx: CoreContext,
    repo: &BlobRepo,
    bcs_id: ChangesetId,
) -> BoxFuture<WalkStep, Error> {
    repo.get_hg_from_bonsai_changeset(ctx, bcs_id)
        .map({
            |hg_cs_id| {
                WalkStep(
                    NodeData::HgChangesetFromBonsaiChangeset(hg_cs_id),
                    vec![Node::HgChangeset(hg_cs_id)],
                )
            }
        })
        .boxify()
}

fn bonsai_changeset_from_hg_step(
    ctx: CoreContext,
    repo: &BlobRepo,
    id: HgChangesetId,
) -> BoxFuture<WalkStep, Error> {
    repo.get_bonsai_from_hg(ctx, id)
        .map(move |maybe_bcs_id| match maybe_bcs_id {
            Some(bcs_id) => {
                let recurse = vec![Node::BonsaiChangeset(bcs_id)];
                WalkStep(
                    NodeData::BonsaiChangesetFromHgChangeset(Some(bcs_id)),
                    recurse,
                )
            }
            None => WalkStep(NodeData::BonsaiChangesetFromHgChangeset(None), vec![]),
        })
        .boxify()
}

fn hg_changeset_step(
    ctx: CoreContext,
    repo: &BlobRepo,
    id: HgChangesetId,
) -> BoxFuture<WalkStep, Error> {
    repo.get_changeset_by_changesetid(ctx, id)
        .map(|hgchangeset| {
            let manifest_id = hgchangeset.manifestid();
            let recurse = vec![Node::HgManifest((None, manifest_id))];
            WalkStep(NodeData::HgChangeset(hgchangeset), recurse)
        })
        .boxify()
}

fn hg_file_envelope_step(
    ctx: CoreContext,
    repo: &BlobRepo,
    hg_file_node_id: HgFileNodeId,
) -> BoxFuture<WalkStep, Error>
where
{
    repo.get_file_envelope(ctx, hg_file_node_id)
        .map({
            move |envelope| {
                let file_content_id = envelope.content_id();
                let fnode = Node::FileContent(file_content_id);
                WalkStep(NodeData::HgFileEnvelope(envelope), vec![fnode])
            }
        })
        .boxify()
}

fn hg_file_node_step(
    ctx: CoreContext,
    repo: &BlobRepo,
    path: Option<MPath>,
    hg_file_node_id: HgFileNodeId,
) -> BoxFuture<WalkStep, Error> {
    let repo_path = match path {
        None => RepoPath::RootPath,
        Some(mpath) => RepoPath::FilePath(mpath),
    };
    repo.get_filenode_opt(ctx, &repo_path, hg_file_node_id)
        .map(move |file_node_opt| match file_node_opt {
            Some(file_node) => {
                // Following linknode increases parallelism of walk
                let linked_commit = Node::BonsaiChangesetFromHgChangeset(file_node.linknode);
                WalkStep(NodeData::HgFileNode(Some(file_node)), vec![linked_commit])
            }
            None => WalkStep(NodeData::HgFileNode(None), vec![]),
        })
        .boxify()
}

fn hg_manifest_step(
    ctx: CoreContext,
    repo: &BlobRepo,
    path: Option<MPath>,
    hg_manifest_id: HgManifestId,
) -> BoxFuture<WalkStep, Error> {
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
                            Node::HgFileEnvelope(hg_file_node_id),
                            Node::HgFileNode((full_path, hg_file_node_id)),
                        ]
                    })
                    .flatten()
                    .collect();

                let mut children_manifests: Vec<_> = manifests
                    .into_iter()
                    .map(move |(full_path, hg_child_manifest_id)| {
                        Node::HgManifest((full_path, hg_child_manifest_id))
                    })
                    .collect();

                children.append(&mut children_manifests);

                WalkStep(NodeData::HgManifest(hgmanifest), children)
            }
        })
        .boxify()
}

/// Expand nodes where check for a type is used as a check for other types.
/// e.g. to make sure metadata looked up/considered for files.
fn expand_checked_nodes(children: &mut Vec<Node>) -> () {
    let mut extra = vec![];
    for n in children.iter() {
        match n {
            Node::FileContent(fc_id) => {
                extra.push(Node::FileContentMetadata(*fc_id));
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

impl Add for StepStats {
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
pub fn walk_exact<FilterItem, NC>(
    ctx: CoreContext,
    repo: BlobRepo,
    walk_roots: Vec<Node>,
    node_checker: NC,
    filter_item: FilterItem,
    scheduled_max: usize,
) -> BoxStream<(Node, Option<(StepStats, NodeData)>), Error>
where
    FilterItem: 'static + Send + Clone + Fn(&Node) -> bool,
    NC: 'static + Clone + NodeChecker + Send,
{
    let changeset_fetcher = repo.get_changeset_fetcher();

    bounded_traversal_stream(scheduled_max, walk_roots, {
        // Each step returns the walk result (e.g. number of blobstore items), and next steps
        move |walk_item| {
            cloned!(ctx);
            let next = match walk_item.clone() {
                Node::Bookmark(bookmark_name) => bookmark_step(ctx, &repo, bookmark_name),
                Node::FileContent(content_id) => file_content_step(ctx, &repo, content_id),
                Node::FileContentMetadata(content_id) => {
                    file_content_metadata_step(ctx, &repo, content_id)
                }
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
                Node::HgChangesetFromBonsaiChangeset(bcs_id) => {
                    hg_changeset_from_bonsai_step(ctx, &repo, bcs_id)
                }
                Node::BonsaiChangesetFromHgChangeset(hg_csid) => {
                    bonsai_changeset_from_hg_step(ctx, &repo, hg_csid)
                }
                Node::BonsaiParents(bcs_id) => {
                    cloned!(changeset_fetcher);
                    bonsai_parents_step(ctx, changeset_fetcher, bcs_id)
                }
                Node::BonsaiChangeset(bcs_id) => bonsai_changeset_step(ctx, &repo, bcs_id),
            }
            .map({
                cloned!(filter_item, node_checker);
                move |WalkStep(nd, mut children)| {
                    children.retain(|c| filter_item.clone()(c));
                    let num_direct = children.len();

                    // Needs to remove before recurse to avoid seeing re-visited nodes in output stream
                    children.retain(|c| node_checker.record_visit(c));
                    let num_direct_new = children.len();

                    expand_checked_nodes(&mut children);
                    // Make sure we don't add in types not wanted
                    children.retain(|c| filter_item(c));
                    let num_expanded_new = children.len();

                    let visited_of_type = node_checker.get_visit_count(&walk_item.get_type());

                    let stats = StepStats {
                        num_direct,
                        num_direct_new,
                        num_expanded_new,
                        visited_of_type,
                    };
                    ((walk_item, Some((stats, nd))), children)
                }
            });
            spawn_future(next)
        }
    })
    .boxify()
}
