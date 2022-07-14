/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use blobrepo::BlobRepo;
use blobstore::Blobstore;
use blobstore::Loadable;
use changesets::ChangesetsRef;
use cloned::cloned;
use context::CoreContext;
use context::PerfCounterType;
use filenodes::FilenodeInfo;
use filenodes::FilenodeRangeResult;
use filenodes::FilenodeResult;
use futures::future;
use futures::future::try_join;
use futures::stream;
use futures::FutureExt;
use futures::Stream;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use maplit::hashset;
use mercurial_types::HgBlobEnvelope;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileHistoryEntry;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgParents;
use mercurial_types::MPath;
use mercurial_types::RepoPath;
use mercurial_types::NULL_CSID;
use mercurial_types::NULL_HASH;
use mononoke_types::ChangesetId;
use slog::debug;
use stats::prelude::*;
use std::collections::HashMap;
use std::collections::VecDeque;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("internal error: file {0} copied from directory {1}")]
    InconsistentCopyInfo(RepoPath, RepoPath),
    #[error("Filenode is missing: {0} {1}")]
    MissingFilenode(RepoPath, HgFileNodeId),
    #[error("Bonsai cs {0} not found")]
    BonsaiNotFound(ChangesetId),
    #[error("Bonsai changeset not found for hg changeset {0}")]
    BonsaiMappingNotFound(HgChangesetId),
}

pub enum FilenodesRelatedResult {
    Unrelated,
    FirstAncestorOfSecond,
    SecondAncestorOfFirst,
}

define_stats! {
    prefix = "mononoke.file_history";
    too_big: dynamic_timeseries("{}.too_big", (repo: String); Rate, Sum),
}

async fn get_filenode_generation(
    ctx: &CoreContext,
    repo: &BlobRepo,
    repo_path: &RepoPath,
    filenode: HgFileNodeId,
) -> Result<Option<u64>, Error> {
    let filenode_info = match repo
        .filenodes()
        .get_filenode(ctx, repo_path, filenode)
        .await?
        .do_not_handle_disabled_filenodes()?
    {
        Some(a) => a,
        None => return Ok(None),
    };
    let linknode = filenode_info.linknode;
    let bcs_id = match repo
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(ctx, linknode)
        .await?
    {
        Some(a) => a,
        None => return Ok(None),
    };
    let bonsai = repo.changesets().get(ctx.clone(), bcs_id).await?;
    Ok(bonsai.map(|b| b.gen))
}

/// Checks if one filenode is ancestor of another
pub async fn check_if_related(
    ctx: CoreContext,
    repo: BlobRepo,
    filenode_a: HgFileNodeId,
    filenode_b: HgFileNodeId,
    path: MPath,
) -> Result<FilenodesRelatedResult, Error> {
    // Use linknodes to identify the older filenode
    let repo_path = RepoPath::file(path.clone())?;
    match try_join(
        get_filenode_generation(&ctx, &repo, &repo_path, filenode_a),
        get_filenode_generation(&ctx, &repo, &repo_path, filenode_b),
    )
    .await?
    {
        // We have linknodes, so know which is the "old" filenode.
        // Halve our data fetches by only fetching full history for the
        // "new" filenode
        (Some(gen_a), Some(gen_b)) => {
            let (old_node, history, relationship) = if gen_a < gen_b {
                (
                    filenode_a,
                    get_file_history(ctx, repo, filenode_b, path, None).await?,
                    FilenodesRelatedResult::FirstAncestorOfSecond,
                )
            } else {
                (
                    filenode_b,
                    get_file_history(ctx, repo, filenode_a, path, None).await?,
                    FilenodesRelatedResult::SecondAncestorOfFirst,
                )
            };

            if let FilenodeResult::Present(history) = history {
                if history.iter().any(|entry| entry.filenode() == &old_node) {
                    Ok(relationship)
                } else {
                    Ok(FilenodesRelatedResult::Unrelated)
                }
            } else {
                Err(anyhow!("filenodes are disabled"))
            }
        }
        // We don't have linknodes, so go down the slow path
        _ => {
            debug!(
                ctx.logger(),
                "No filenodes for parents. Using slow path for ancestry check"
            );
            match try_join(
                get_file_history(ctx.clone(), repo.clone(), filenode_a, path.clone(), None),
                get_file_history(ctx, repo, filenode_b, path, None),
            )
            .await?
            {
                (FilenodeResult::Present(history_a), FilenodeResult::Present(history_b)) => {
                    if history_a
                        .iter()
                        .any(|entry| entry.filenode() == &filenode_b)
                    {
                        Ok(FilenodesRelatedResult::SecondAncestorOfFirst)
                    } else if history_b
                        .iter()
                        .any(|entry| entry.filenode() == &filenode_a)
                    {
                        Ok(FilenodesRelatedResult::FirstAncestorOfSecond)
                    } else {
                        Ok(FilenodesRelatedResult::Unrelated)
                    }
                }
                _ => Err(anyhow!("filenodes are disabled")),
            }
        }
    }
}

/// Same as get_file_history(), but returns incomplete history if filenodes
/// are disabled
pub fn get_file_history_maybe_incomplete(
    ctx: CoreContext,
    repo: BlobRepo,
    filenode: HgFileNodeId,
    path: MPath,
    max_length: Option<u64>,
) -> impl Stream<Item = Result<HgFileHistoryEntry, Error>> {
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
                FilenodeResult::Present(file_history) => future::ok(file_history).left_future(),
                FilenodeResult::Disabled => async move {
                    // Filenodes are disabled - fetch a single filenode
                    // from a blobstore
                    let path = RepoPath::FilePath(path);
                    let filenode_info = get_filenode_from_envelope(
                        repo.get_blobstore(),
                        &ctx,
                        &path,
                        filenode,
                        NULL_CSID,
                    )
                    .await?;
                    let filenode = filenode_to_history_entry(filenode, filenode_info, &path)?;
                    Ok(vec![filenode])
                }
                .right_future(),
            }
        }
    })
    .map_ok(|hist| stream::iter(hist.into_iter().map(Ok)))
    .try_flatten_stream()
}

/// Get the history of the file corresponding to the given filenode and path.
pub async fn get_file_history(
    ctx: CoreContext,
    repo: BlobRepo,
    filenode: HgFileNodeId,
    path: MPath,
    max_length: Option<u64>,
) -> Result<FilenodeResult<Vec<HgFileHistoryEntry>>, Error> {
    let prefetched_res = prefetch_history(&ctx, &repo, path.clone(), max_length).await?;
    match prefetched_res {
        FilenodeRangeResult::Present(prefetched) => {
            let history = get_file_history_using_prefetched(
                ctx, repo, filenode, path, max_length, prefetched,
            )
            .try_collect()
            .await?;
            Ok(FilenodeResult::Present(history))
        }
        FilenodeRangeResult::TooBig => {
            ctx.perf_counters()
                .increment_counter(PerfCounterType::FilenodesTooBigHistory);
            STATS::too_big.add_value(1, (repo.name().clone(),));
            let history = get_file_history_using_prefetched(
                ctx,
                repo,
                filenode,
                path,
                max_length,
                HashMap::new(),
            )
            .try_collect()
            .await?;
            Ok(FilenodeResult::Present(history))
        }
        FilenodeRangeResult::Disabled => Ok(FilenodeResult::Disabled),
    }
}

/// Prefetch and cache filenode information. Performing these fetches in bulk upfront
/// prevents an excessive number of DB roundtrips when constructing file history.
async fn prefetch_history(
    ctx: &CoreContext,
    repo: &BlobRepo,
    path: MPath,
    limit: Option<u64>,
) -> Result<FilenodeRangeResult<HashMap<HgFileNodeId, FilenodeInfo>>, Error> {
    let filenodes_res = repo
        .filenodes()
        .get_all_filenodes_maybe_stale(ctx, &RepoPath::FilePath(path), limit)
        .await?;
    Ok(filenodes_res.map(|filenodes| {
        filenodes
            .into_iter()
            .map(|filenode| (filenode.filenode, filenode))
            .collect()
    }))
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
    max_length: Option<u64>,
    prefetched_history: HashMap<HgFileNodeId, FilenodeInfo>,
) -> impl Stream<Item = Result<HgFileHistoryEntry, Error>> {
    if startnode == HgFileNodeId::new(NULL_HASH) {
        return stream::empty().left_stream();
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

async fn get_maybe_missing_filenode(
    ctx: &CoreContext,
    repo: &BlobRepo,
    path: &RepoPath,
    node: HgFileNodeId,
) -> Result<FilenodeInfo, Error> {
    let filenode_res = repo.filenodes().get_filenode(ctx, path, node).await?;
    match filenode_res {
        FilenodeResult::Present(Some(filenode)) => Ok(filenode),
        FilenodeResult::Present(None) | FilenodeResult::Disabled => {
            // The filenode couldn't be found.  This may be because it is a
            // draft node, which doesn't get stored in the database or because
            // filenodes were intentionally disabled.  Attempt
            // to reconstruct the filenode from the envelope.  Use `NULL_CSID`
            // to indicate a draft or missing linknode.
            get_filenode_from_envelope(repo.get_blobstore(), ctx, path, node, NULL_CSID).await
        }
    }
}

async fn get_filenode_from_envelope(
    blobstore: impl Blobstore + 'static,
    ctx: &CoreContext,
    path: &RepoPath,
    node: HgFileNodeId,
    linknode: HgChangesetId,
) -> Result<FilenodeInfo, Error> {
    let envelope = node.load(ctx, &blobstore).await.with_context({
        cloned!(path);
        move || format!("While fetching filenode for {} {}", path, node)
    })?;
    let (p1, p2) = envelope.parents();
    let copyfrom = envelope
        .get_copy_info()
        .with_context({
            cloned!(path);
            move || format!("While parsing copy information for {} {}", path, node)
        })?
        .map(|(path, node)| (RepoPath::FilePath(path), node));
    Ok(FilenodeInfo {
        filenode: node,
        p1,
        p2,
        copyfrom,
        linknode,
    })
}
