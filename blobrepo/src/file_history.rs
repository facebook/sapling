/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::collections::{HashMap, HashSet, VecDeque};

use crate::repo::BlobRepo;
use cloned::cloned;
use context::CoreContext;
use failure_ext::{Error, Fail};
use filenodes::FilenodeInfo;
use futures::{future::ok, stream, Future, Stream};
use futures_ext::{BoxStream, FutureExt, StreamExt};
use maplit::hashset;
use mercurial_types::{
    HgFileHistoryEntry, HgFileNodeId, HgParents, MPath, RepoPath, NULL_CSID, NULL_HASH,
};

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "internal error: file {} copied from directory {}", _0, _1)]
    InconsistentCopyInfo(RepoPath, RepoPath),
}

pub enum FilenodesRelatedResult {
    Unrelated,
    FirstAncestorOfSecond,
    SecondAncestorOfFirst,
}

/// Checks if one filenode is ancestor of another
pub fn check_if_related(
    ctx: CoreContext,
    repo: BlobRepo,
    filenode_a: HgFileNodeId,
    filenode_b: HgFileNodeId,
    path: MPath,
) -> impl Future<Item = FilenodesRelatedResult, Error = Error> {
    get_file_history(
        ctx.clone(),
        repo.clone(),
        filenode_a.clone(),
        path.clone(),
        None,
    )
    .collect()
    .join(
        get_file_history(
            ctx.clone(),
            repo.clone(),
            filenode_b.clone(),
            path.clone(),
            None,
        )
        .collect(),
    )
    .map(move |(history_a, history_b)| {
        if history_a
            .iter()
            .any(|entry| entry.filenode() == &filenode_b)
        {
            FilenodesRelatedResult::SecondAncestorOfFirst
        } else if history_b
            .iter()
            .any(|entry| entry.filenode() == &filenode_a)
        {
            FilenodesRelatedResult::FirstAncestorOfSecond
        } else {
            FilenodesRelatedResult::Unrelated
        }
    })
}

/// Get the history of the file corresponding to the given filenode and path.
pub fn get_file_history(
    ctx: CoreContext,
    repo: BlobRepo,
    filenode: HgFileNodeId,
    path: MPath,
    max_depth: Option<u32>,
) -> impl Stream<Item = HgFileHistoryEntry, Error = Error> {
    prefetch_history(ctx.clone(), repo.clone(), path.clone())
        .map(move |prefetched| {
            get_file_history_using_prefetched(ctx, repo, filenode, path, max_depth, prefetched)
        })
        .flatten_stream()
}

/// Prefetch and cache filenode information. Performing these fetches in bulk upfront
/// prevents an excessive number of DB roundtrips when constructing file history.
fn prefetch_history(
    ctx: CoreContext,
    repo: BlobRepo,
    path: MPath,
) -> impl Future<Item = HashMap<HgFileNodeId, FilenodeInfo>, Error = Error> {
    repo.get_all_filenodes_maybe_stale(ctx, RepoPath::FilePath(path))
        .map(|filenodes| {
            filenodes
                .into_iter()
                .map(|filenode| (filenode.filenode, filenode))
                .collect()
        })
}

/// Get the history of the file at the specified path, using the given
/// prefetched history map as a cache to speed up the operation.
fn get_file_history_using_prefetched(
    ctx: CoreContext,
    repo: BlobRepo,
    startnode: HgFileNodeId,
    path: MPath,
    max_depth: Option<u32>,
    prefetched_history: HashMap<HgFileNodeId, FilenodeInfo>,
) -> BoxStream<HgFileHistoryEntry, Error> {
    if startnode == HgFileNodeId::new(NULL_HASH) {
        return stream::empty().boxify();
    }

    let mut startstate = VecDeque::new();
    startstate.push_back(startnode);
    let seen_nodes = hashset! {startnode};
    let path = RepoPath::FilePath(path);

    stream::unfold(
        (startstate, seen_nodes, 0),
        move |(mut nodes, mut seen_nodes, depth): (
            VecDeque<HgFileNodeId>,
            HashSet<HgFileNodeId>,
            u32,
        )| {
            match max_depth {
                Some(max_depth) if depth >= max_depth => return None,
                _ => {}
            }

            let node = nodes.pop_front()?;

            let filenode_fut = if let Some(filenode) = prefetched_history.get(&node) {
                ok(filenode.clone()).left_future()
            } else {
                get_maybe_draft_filenode(ctx.clone(), repo.clone(), path.clone(), node)
                    .right_future()
            };

            let history = filenode_fut.and_then(move |filenode| {
                let p1 = filenode.p1.map(|p| p.into_nodehash());
                let p2 = filenode.p2.map(|p| p.into_nodehash());
                let parents = HgParents::new(p1, p2);

                let linknode = filenode.linknode;

                let copyfrom = match filenode.copyfrom {
                    Some((RepoPath::FilePath(frompath), node)) => Some((frompath, node)),
                    Some((frompath, _)) => {
                        return Err(ErrorKind::InconsistentCopyInfo(filenode.path, frompath).into());
                    }
                    None => None,
                };

                let entry = HgFileHistoryEntry::new(node, parents, linknode, copyfrom);

                nodes.extend(
                    parents
                        .into_iter()
                        .map(HgFileNodeId::new)
                        .filter(|p| seen_nodes.insert(*p)),
                );
                Ok((entry, (nodes, seen_nodes, depth + 1)))
            });

            Some(history)
        },
    )
    .boxify()
}

pub fn get_maybe_draft_filenode(
    ctx: CoreContext,
    repo: BlobRepo,
    path: RepoPath,
    node: HgFileNodeId,
) -> impl Future<Item = FilenodeInfo, Error = Error> {
    repo.get_filenode_opt(ctx.clone(), &path, node).and_then({
        cloned!(repo, ctx, path, node);
        move |filenode_opt| match filenode_opt {
            Some(filenode) => ok(filenode).left_future(),
            None => {
                // The filenode couldn't be found.  This may be because it is a
                // draft node, which doesn't get stored in the database.  Attempt
                // to reconstruct the filenode from the envelope.  Use `NULL_CSID`
                // to indicate a draft linknode.
                repo.get_filenode_from_envelope(ctx, &path, node, NULL_CSID)
                    .right_future()
            }
        }
    })
}
