/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::graph::{FileContentData, Node, NodeData};
use blobrepo::BlobRepo;
use bookmarks::BookmarkName;
use changeset_fetcher::ChangesetFetcher;
use cloned::cloned;
use context::CoreContext;
use failure_ext::{format_err, Error};
use futures::{
    future::{self},
    Future, Stream,
};
use futures_ext::{bounded_traversal::bounded_traversal_stream, BoxFuture, FutureExt};
use itertools::{Either, Itertools};
use mercurial_types::{
    Changeset, HgChangesetId, HgEntryId, HgFileNodeId, HgManifest, HgManifestId, RepoPath,
};
use mononoke_types::{ChangesetId, ContentId, MPath};
use std::{iter::IntoIterator, sync::Arc};

pub trait NodeChecker {
    // This is a simple check, no change to internal state
    fn has_visited(self: &Self, n: &Node) -> bool;

    // This can mutate the internal state.  Returns true if we should visit the node
    fn record_visit(self: &mut Self, n: &Node) -> bool;
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

fn bonsai_changeset_step<NC>(
    ctx: CoreContext,
    repo: &BlobRepo,
    node_checker: NC,
    bcs_id: ChangesetId,
) -> BoxFuture<WalkStep, Error>
where
    NC: 'static + Send + Clone + NodeChecker,
{
    // Get the data, and add direct file data for this bonsai changeset
    let bonsai_fut = repo
        .get_bonsai_changeset(ctx.clone(), bcs_id)
        .map({
            cloned!(node_checker);
            move |bcs| {
                let files_to_visit: Vec<Node> = bcs
                    .file_changes()
                    .filter_map(|(_mpath, fc_opt)| {
                        fc_opt // remove None
                    })
                    .map(|fc| {
                        vec![
                            Node::FileContent(fc.content_id()),
                            Node::FileContentMetadata(fc.content_id()),
                        ]
                    })
                    .filter(|fc| !node_checker.has_visited(&fc[0]))
                    .flatten()
                    .collect();
                (bcs, files_to_visit)
            }
        })
        .boxify();

    bonsai_fut
        .map(move |(bcs, mut children)| {
            // Allow Hg based lookup
            children.push(Node::HgChangesetFromBonsaiChangeset(bcs_id));
            // Parents deliberately last to encourage wide walk when
            // one of the manifest expansions is on
            children.push(Node::BonsaiParents(bcs_id));
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
        .get_parents(ctx.clone(), bcs_id)
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
    repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id)
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

fn hg_changeset_step(
    ctx: CoreContext,
    repo: &BlobRepo,
    id: HgChangesetId,
) -> BoxFuture<WalkStep, Error> {
    repo.get_changeset_by_changesetid(ctx, id)
        .map(|hgchangeset| {
            // TODO - check assumption: this is root manifest for this change as no path known yet.
            let manifest_id = hgchangeset.manifestid();
            let recurse = vec![Node::HgManifest((None, manifest_id))];
            WalkStep(NodeData::HgChangeset(hgchangeset), recurse)
        })
        .boxify()
}

fn hg_file_envelope_step<NC>(
    ctx: CoreContext,
    repo: &BlobRepo,
    node_checker: NC,
    hg_file_node_id: HgFileNodeId,
) -> BoxFuture<WalkStep, Error>
where
    NC: 'static + Send + Clone + NodeChecker,
{
    repo.get_file_envelope(ctx, hg_file_node_id)
        .map({
            cloned!(node_checker);
            move |envelope| {
                let file_content_id = envelope.content_id();
                let fnode = Node::FileContent(file_content_id);
                let recurse = if !node_checker.has_visited(&fnode) {
                    vec![fnode, Node::FileContentMetadata(file_content_id)]
                } else {
                    vec![]
                };
                WalkStep(NodeData::HgFileEnvelope(envelope), recurse)
            }
        })
        .boxify()
}

fn hg_file_node_step(
    ctx: CoreContext,
    repo: &BlobRepo,
    path: &RepoPath,
    hg_file_node_id: HgFileNodeId,
) -> BoxFuture<WalkStep, Error> {
    repo.get_filenode_opt(ctx, path, hg_file_node_id)
        .map(move |file_node_opt| WalkStep(NodeData::HgFileNode(file_node_opt), vec![]))
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
                        let full_path = MPath::join_element_opt(path.as_ref(), child.get_name());
                        match child.get_hash() {
                            HgEntryId::File(_, filenode_id) => {
                                Either::Right((full_path, filenode_id.clone()))
                            }
                            HgEntryId::Manifest(manifest_id) => {
                                Either::Left((full_path, manifest_id.clone()))
                            }
                        }
                    });

                let mut children: Vec<_> = filenodes
                    .into_iter()
                    .map(move |(full_path, hg_file_node_id)| match full_path {
                        Some(fpath) => vec![
                            Node::HgFileEnvelope(hg_file_node_id),
                            Node::HgFileNode((RepoPath::FilePath(fpath), hg_file_node_id)),
                        ],
                        None => vec![Node::HgFileEnvelope(hg_file_node_id)],
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

/// Walk the graph from one or more starting points,  providing stream of data for later reduction
pub fn walk_exact<FilterItem, NC>(
    ctx: CoreContext,
    repo: BlobRepo,
    walk_roots: Vec<Node>,
    node_checker: NC,
    filter_item: FilterItem,
    scheduled_max: usize,
) -> impl Stream<Item = (Node, Option<NodeData>), Error = Error>
where
    FilterItem: 'static + Send + Fn(&Node) -> bool,
    NC: 'static + Send + Clone + NodeChecker,
{
    let changeset_fetcher = repo.get_changeset_fetcher();

    bounded_traversal_stream(scheduled_max, walk_roots, {
        // Each step returns the walk result (e.g. number of blobstore items), and next steps
        move |walk_item| {
            if !filter_item(&walk_item) {
                return future::ok(((walk_item, None), vec![])).boxify();
            }
            cloned!(ctx);
            match walk_item.clone() {
                Node::Bookmark(bookmark_name) => bookmark_step(ctx, &repo, bookmark_name),
                Node::FileContent(content_id) => file_content_step(ctx, &repo, content_id),
                Node::FileContentMetadata(content_id) => {
                    file_content_metadata_step(ctx, &repo, content_id)
                }
                Node::HgChangeset(hg_csid) => hg_changeset_step(ctx, &repo, hg_csid),
                Node::HgFileEnvelope(hg_file_node_id) => {
                    cloned!(node_checker);
                    hg_file_envelope_step(ctx, &repo, node_checker, hg_file_node_id)
                }
                Node::HgFileNode((path, hg_file_node_id)) => {
                    hg_file_node_step(ctx, &repo, &path, hg_file_node_id)
                }
                Node::HgManifest((path, hg_manifest_id)) => {
                    hg_manifest_step(ctx, &repo, path, hg_manifest_id)
                }
                Node::HgChangesetFromBonsaiChangeset(bcs_id) => {
                    hg_changeset_from_bonsai_step(ctx, &repo, bcs_id)
                }
                Node::BonsaiParents(bcs_id) => {
                    cloned!(changeset_fetcher);
                    bonsai_parents_step(ctx, changeset_fetcher, bcs_id)
                }
                Node::BonsaiChangeset(bcs_id) => {
                    cloned!(node_checker);
                    bonsai_changeset_step(ctx, &repo, node_checker, bcs_id)
                }
            }
            .map({
                cloned!(mut node_checker);
                move |WalkStep(nd, mut children)| {
                    // Needs to remove before recurse to avoid seeing re-visited nodes in output stream
                    children.retain(|c| node_checker.record_visit(c));
                    ((walk_item, Some(nd)), children)
                }
            })
            .boxify()
        }
    })
}
