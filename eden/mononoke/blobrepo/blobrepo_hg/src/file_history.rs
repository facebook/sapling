/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::BlobRepoHg;
use anyhow::{anyhow, Error};
use blobrepo::BlobRepo;
use cloned::cloned;
use context::CoreContext;
use filenodes::{FilenodeInfo, FilenodeResult};
use futures::{
    compat::Future01CompatExt,
    stream,
    stream::{StreamExt, TryStreamExt},
};
use futures_ext::{FutureExt, StreamExt as OldStreamExt};
use futures_old::{
    future::{err, ok},
    stream as old_stream, Future, Stream,
};
use maplit::hashset;
use mercurial_types::{
    HgFileHistoryEntry, HgFileNodeId, HgParents, MPath, RepoPath, NULL_CSID, NULL_HASH,
};
use std::collections::{HashMap, VecDeque};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("internal error: file {0} copied from directory {1}")]
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
    .join(get_file_history(ctx, repo, filenode_b.clone(), path, None))
    .and_then(move |(history_a, history_b)| match (history_a, history_b) {
        (FilenodeResult::Present(history_a), FilenodeResult::Present(history_b)) => {
            if history_a
                .iter()
                .any(|entry| entry.filenode() == &filenode_b)
            {
                ok(FilenodesRelatedResult::SecondAncestorOfFirst).left_future()
            } else if history_b
                .iter()
                .any(|entry| entry.filenode() == &filenode_a)
            {
                ok(FilenodesRelatedResult::FirstAncestorOfSecond).left_future()
            } else {
                ok(FilenodesRelatedResult::Unrelated).left_future()
            }
        }
        _ => err(anyhow!("filenodes are disabled")).right_future(),
    })
}

/// Same as get_file_history(), but returns incomplete history if filenodes
/// are disabled
pub fn get_file_history_maybe_incomplete(
    ctx: CoreContext,
    repo: BlobRepo,
    filenode: HgFileNodeId,
    path: MPath,
    max_length: Option<u32>,
) -> impl Stream<Item = HgFileHistoryEntry, Error = Error> {
    get_file_history(
        ctx.clone(),
        repo.clone(),
        filenode,
        path.clone(),
        max_length,
    )
    .and_then({
        cloned!(ctx, path, repo);
        move |file_history_res| {
            match file_history_res {
                FilenodeResult::Present(file_history) => ok(file_history).left_future(),
                FilenodeResult::Disabled => {
                    // Filenodes are disabled - fetch a single filenode
                    // from a blobstore
                    let path = RepoPath::FilePath(path);
                    repo.get_filenode_from_envelope(ctx.clone(), &path, filenode, NULL_CSID)
                        .and_then(move |filenode_info| {
                            let filenode =
                                filenode_to_history_entry(filenode, filenode_info, &path)?;
                            Ok(vec![filenode])
                        })
                        .right_future()
                }
            }
        }
    })
    .map(old_stream::iter_ok)
    .flatten_stream()
}

/// Get the history of the file corresponding to the given filenode and path.
pub fn get_file_history(
    ctx: CoreContext,
    repo: BlobRepo,
    filenode: HgFileNodeId,
    path: MPath,
    max_length: Option<u32>,
) -> impl Future<Item = FilenodeResult<Vec<HgFileHistoryEntry>>, Error = Error> {
    prefetch_history(ctx.clone(), repo.clone(), path.clone()).and_then(move |prefetched_res| {
        match prefetched_res {
            FilenodeResult::Present(prefetched) => {
                get_file_history_using_prefetched(ctx, repo, filenode, path, max_length, prefetched)
                    .collect()
                    .map(FilenodeResult::Present)
                    .left_future()
            }
            FilenodeResult::Disabled => ok(FilenodeResult::Disabled).right_future(),
        }
    })
}

/// Prefetch and cache filenode information. Performing these fetches in bulk upfront
/// prevents an excessive number of DB roundtrips when constructing file history.
fn prefetch_history(
    ctx: CoreContext,
    repo: BlobRepo,
    path: MPath,
) -> impl Future<Item = FilenodeResult<HashMap<HgFileNodeId, FilenodeInfo>>, Error = Error> {
    repo.get_all_filenodes_maybe_stale(ctx, RepoPath::FilePath(path))
        .map(|filenodes_res| {
            filenodes_res.map(|filenodes| {
                filenodes
                    .into_iter()
                    .map(|filenode| (filenode.filenode, filenode))
                    .collect()
            })
        })
}

/// Get the history of the file at the specified path, using the given
/// prefetched history map as a cache to speed up the operation.
///
/// FIXME: max_legth parameter is not necessary. We can use .take() method on the stream
/// i.e. get_file_history_using_prefetched().take(max_length)
fn get_file_history_using_prefetched(
    ctx: CoreContext,
    repo: BlobRepo,
    startnode: HgFileNodeId,
    path: MPath,
    max_length: Option<u32>,
    prefetched_history: HashMap<HgFileNodeId, FilenodeInfo>,
) -> impl Stream<Item = HgFileHistoryEntry, Error = Error> {
    if startnode == HgFileNodeId::new(NULL_HASH) {
        return old_stream::empty().left_stream();
    }

    let mut startstate = VecDeque::new();
    startstate.push_back(startnode);
    let seen_nodes = hashset! {startnode};
    let path = RepoPath::FilePath(path);

    struct BfsContext {
        ctx: CoreContext,
        repo: BlobRepo,
        path: RepoPath,
        prefetched_history: HashMap<HgFileNodeId, FilenodeInfo>,
    }

    let bfs_context = BfsContext {
        ctx,
        repo,
        path,
        prefetched_history,
    };

    // TODO: There is probably another thundering herd problem here. If we change a file twice,
    // then the original cached value will be reused, and we'll keep going back to getting the
    // filenode individualy (perhaps not the end of the world?).
    stream::try_unfold(
        (bfs_context, startstate, seen_nodes, 0),
        move |(bfs_context, mut nodes, mut seen_nodes, length)| async move {
            match max_length {
                Some(max_length) if length >= max_length => return Ok(None),
                _ => {}
            }

            let node = match nodes.pop_front() {
                Some(node) => node,
                None => {
                    return Ok(None);
                }
            };

            let filenode = if let Some(filenode) = bfs_context.prefetched_history.get(&node) {
                filenode.clone()
            } else {
                get_maybe_missing_filenode(
                    &bfs_context.ctx,
                    &bfs_context.repo,
                    &bfs_context.path,
                    node,
                )
                .compat()
                .await?
            };

            let p1 = filenode.p1.map(|p| p.into_nodehash());
            let p2 = filenode.p2.map(|p| p.into_nodehash());
            let parents = HgParents::new(p1, p2);
            let entry = filenode_to_history_entry(node, filenode, &bfs_context.path)?;

            nodes.extend(
                parents
                    .into_iter()
                    .map(HgFileNodeId::new)
                    .filter(|p| seen_nodes.insert(*p)),
            );

            Ok(Some((entry, (bfs_context, nodes, seen_nodes, length + 1))))
        },
    )
    .boxed()
    .compat()
    .right_stream()
}

pub fn filenode_to_history_entry(
    node: HgFileNodeId,
    filenode: FilenodeInfo,
    path: &RepoPath,
) -> Result<HgFileHistoryEntry, Error> {
    let p1 = filenode.p1.map(|p| p.into_nodehash());
    let p2 = filenode.p2.map(|p| p.into_nodehash());
    let parents = HgParents::new(p1, p2);

    let linknode = filenode.linknode;

    let copyfrom = match filenode.copyfrom {
        Some((RepoPath::FilePath(frompath), node)) => Some((frompath, node)),
        Some((frompath, _)) => {
            return Err(ErrorKind::InconsistentCopyInfo(path.clone(), frompath).into());
        }
        None => None,
    };

    Ok(HgFileHistoryEntry::new(node, parents, linknode, copyfrom))
}

fn get_maybe_missing_filenode(
    ctx: &CoreContext,
    repo: &BlobRepo,
    path: &RepoPath,
    node: HgFileNodeId,
) -> impl Future<Item = FilenodeInfo, Error = Error> {
    repo.get_filenode_opt(ctx.clone(), path, node).and_then({
        cloned!(repo, ctx, path, node);
        move |filenode_res| match filenode_res {
            FilenodeResult::Present(Some(filenode)) => ok(filenode).left_future(),
            FilenodeResult::Present(None) | FilenodeResult::Disabled => {
                // The filenode couldn't be found.  This may be because it is a
                // draft node, which doesn't get stored in the database or because
                // filenodes were intentionally disabled.  Attempt
                // to reconstruct the filenode from the envelope.  Use `NULL_CSID`
                // to indicate a draft or missing linknode.
                repo.get_filenode_from_envelope(ctx, &path, node, NULL_CSID)
                    .right_future()
            }
        }
    })
}
