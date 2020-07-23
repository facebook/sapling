/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Error};
use async_trait::async_trait;
use blobrepo::BlobRepo;
use blobstore::{Blobstore, Loadable, LoadableError};
use cloned::cloned;
use context::CoreContext;
use deleted_files_manifest::{resolve_path_state, PathState};
use derived_data::{BonsaiDerived, DeriveError};
use futures::{
    compat::Future01CompatExt,
    future,
    stream::{self, Stream as NewStream},
};
use futures_stats::futures03::TimedFutureExt;
use futures_util::{StreamExt, TryStreamExt};
use itertools::Itertools;
use manifest::{Entry, ManifestOps};
use mononoke_types::{ChangesetId, FileUnodeId, MPath, ManifestUnodeId};
use stats::prelude::*;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use thiserror::Error;
use time_ext::DurationExt;
use unodes::RootUnodeManifestId;

use crate::fastlog_impl::{fetch_fastlog_batch_by_unode_id, fetch_flattened};
use crate::mapping::{FastlogParent, RootFastlog};

define_stats! {
    prefix = "mononoke.fastlog";
    unexpected_existing_unode: timeseries(Sum),
    find_where_file_was_deleted_ms: timeseries(Sum, Average),
}

#[derive(Debug, Error)]
pub enum FastlogError {
    #[error("No such path: {0}")]
    NoSuchPath(MPath),
    #[error("Internal error: {0}")]
    InternalError(String),
    #[error("{0}")]
    DeriveError(#[from] DeriveError),
    #[error("{0}")]
    LoadableError(#[from] LoadableError),
    #[error("{0}")]
    Error(#[from] Error),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum HistoryAcrossDeletions {
    Track,
    DontTrack,
}

/// Returns a full history of the given path starting from the given unode in BFS order.
///
/// Accepts a `Visitor` object which controls the BFS flow by filtering out the unwanted changesets
/// before they're added to the queue, see its docs for details. If you don't need to filter the
/// history you can provide `()` instead for default implementation.
///
/// This is the public API of this crate i.e. what clients should use if they want to
/// fetch the history.
///
/// Given a unode representing a commit-path `list_file_history` traverses commit history
/// in BFS order.
/// In order to do this it keeps:
///   - history_graph: commit graph that is constructed from fastlog data and represents
///                    'child(cs_id) -> parents(cs_id)' relationship
///   - prefetch: changeset id to fetch fastlog batch for
///   - bfs: BFS queue
///   - visited: set that marks nodes already enqueued for BFS
/// For example, for this commit graph where some file is changed in every commit and E - start:
///
///      o E  - stage: 0        commit_graph: E -> D
///      |                                    D -> B, C
///      o D  - stage: 1                      C -> []
///     / \                                   B -> A
///  B o  o C - stage: 2                      A -> []
///    |
///    o A    - stage: 3
///
/// On each step of try_unfold:
///   1 - prefetch fastlog batch for the `prefetch` changeset id and fill the commit graph
///   2 - perform BFS until the node for which parents haven't been prefetched
///   3 - stream all the "ready" nodes and set the last node to prefetch
/// The stream stops when there is nothing to return.
///
/// Why to pop all nodes on the same depth and not just one commit at a time?
/// Because if history contains merges and parents for more than one node on the current depth
/// haven't been fetched yet, we can fetch them at the same time using FuturesUnordered.
pub async fn list_file_history(
    ctx: CoreContext,
    repo: BlobRepo,
    path: Option<MPath>,
    changeset_id: ChangesetId,
    mut visitor: impl Visitor,
    history_across_deletions: HistoryAcrossDeletions,
) -> Result<impl NewStream<Item = Result<ChangesetId, Error>>, FastlogError> {
    let mut top_history = vec![];
    // get unode entry
    let not_found_err = || {
        if let Some(p) = path.clone() {
            FastlogError::NoSuchPath(p)
        } else {
            FastlogError::InternalError("cannot find unode for the repo root".to_string())
        }
    };
    let resolved = resolve_path_state(&ctx, &repo, changeset_id, &path).await?;

    let mut visited = HashSet::new();
    let mut history_graph = HashMap::new();

    // there might be more than one unode entry: if the given path was
    // deleted in several different branches, we have to gather history
    // from all of them
    let last_changesets = match resolved {
        Some(PathState::Deleted(deletion_nodes)) => {
            // we want to show commit, where path was deleted
            process_deletion_nodes(&ctx, &repo, &mut history_graph, deletion_nodes).await?
        }
        Some(PathState::Exists(unode_entry)) => {
            fetch_linknodes_and_update_graph(&ctx, &repo, vec![unode_entry], &mut history_graph)
                .await?
        }
        None => {
            return Err(not_found_err());
        }
    };

    let mut bfs = VecDeque::new();
    visit(
        &ctx,
        &repo,
        &mut visitor,
        None,
        last_changesets.clone(),
        &mut bfs,
        &mut visited,
        &mut top_history,
    )
    .await?;

    // generate file history
    Ok(stream::iter(top_history)
        .map(Ok::<_, Error>)
        .chain({
            stream::try_unfold(
                // starting point
                TraversalState {
                    history_graph,
                    visited,
                    bfs,
                    prefetch: None,
                    visitor,
                },
                // unfold
                move |state| {
                    cloned!(ctx, repo, path);
                    async move {
                        do_history_unfold(
                            ctx.clone(),
                            repo.clone(),
                            path.clone(),
                            state,
                            history_across_deletions,
                        )
                        .await
                    }
                },
            )
            .map_ok(|history| stream::iter(history).map(Ok))
            .try_flatten()
        })
        .boxed())
}

#[async_trait]
pub trait Visitor: Send + 'static {
    /// Filters out the changesets which should be pursued during BFS traversal of file history.
    ///
    /// Given `cs_ids` list returns the filtered list of changesets that should be part of the
    /// traversal result and which should be pursues recursively.
    ///
    /// `descendant_cs_id` is:
    ///  * None in the first BFS iteration
    ///  * common descendant of the ancestors that lead us to processing them.
    async fn visit(
        &mut self,
        ctx: &CoreContext,
        repo: &BlobRepo,
        descendant_cs_id: Option<ChangesetId>,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<Vec<ChangesetId>, Error>;

    /// May be called before visiting node so the visitor can prefetch neccesary
    /// data to make the visit faster.
    ///
    /// This funtion is not guaranteed to be called before each visit() call.
    //  The visit() is not guaranteed to be called later -  the traversal may terminat earlier.
    async fn preprocess(
        &mut self,
        _ctx: &CoreContext,
        _repo: &BlobRepo,
        _descendant_id_cs_ids: Vec<(Option<ChangesetId>, Vec<ChangesetId>)>,
    ) -> Result<(), Error> {
        Ok(())
    }
}

#[async_trait]
impl Visitor for () {
    async fn visit(
        &mut self,
        _ctx: &CoreContext,
        _repo: &BlobRepo,
        _descentant_cs_id: Option<ChangesetId>,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<Vec<ChangesetId>, Error> {
        Ok(cs_ids)
    }
}

// Encapsulates all the things that should happen when the ancestors of a single history
// node are processed.
async fn visit(
    ctx: &CoreContext,
    repo: &BlobRepo,
    visitor: &mut impl Visitor,
    cs_id: Option<ChangesetId>,
    ancestors: Vec<ChangesetId>,
    bfs: &mut VecDeque<ChangesetId>,
    visited: &mut HashSet<ChangesetId>,
    history: &mut Vec<ChangesetId>,
) -> Result<(), FastlogError> {
    let ancestors = visitor.visit(ctx, repo, cs_id, ancestors).await?;
    for ancestor in ancestors {
        if visited.insert(ancestor) {
            history.push(ancestor.clone());
            bfs.push_back(ancestor);
        }
    }
    Ok(())
}

type UnodeEntry = Entry<ManifestUnodeId, FileUnodeId>;

// Resolves the deletion nodes and inserts them into history as-if they were normal
// nodes being part of fastlog batch.
async fn process_deletion_nodes(
    ctx: &CoreContext,
    repo: &BlobRepo,
    history_graph: &mut CommitGraph,
    deletion_nodes: Vec<(ChangesetId, UnodeEntry)>,
) -> Result<Vec<ChangesetId>, FastlogError> {
    let mut deleted_linknodes = vec![];
    let mut last_unodes = vec![];

    for (deleted_linknode, last_unode_entry) in deletion_nodes {
        deleted_linknodes.push(deleted_linknode);
        last_unodes.push(last_unode_entry);
    }

    let last_linknodes =
        fetch_linknodes_and_update_graph(&ctx, &repo, last_unodes, history_graph).await?;
    let mut deleted_to_last_mapping: Vec<_> = deleted_linknodes
        .iter()
        .zip(last_linknodes.into_iter())
        .collect();
    deleted_to_last_mapping.sort_by_key(|(deleted_linknode, _)| *deleted_linknode);
    deleted_to_last_mapping
        .into_iter()
        .group_by(|(deleted_linknode, _)| **deleted_linknode)
        .into_iter()
        .for_each(|(deleted_linknode, grouped_last)| {
            history_graph.insert(
                deleted_linknode,
                Some(grouped_last.map(|(_, last)| last).collect()),
            );
        });
    Ok(deleted_linknodes)
}

async fn fetch_linknodes_and_update_graph(
    ctx: &CoreContext,
    repo: &BlobRepo,
    unode_entries: Vec<UnodeEntry>,
    history_graph: &mut CommitGraph,
) -> Result<Vec<ChangesetId>, FastlogError> {
    let linknodes = unode_entries.into_iter().map({
        cloned!(ctx, repo);
        move |entry| {
            cloned!(ctx, repo);
            async move {
                let unode = entry.load(ctx.clone(), &repo.get_blobstore()).await?;
                Ok::<_, FastlogError>(match unode {
                    Entry::Tree(mf_unode) => mf_unode.linknode().clone(),
                    Entry::Leaf(file_unode) => file_unode.linknode().clone(),
                })
            }
        }
    });
    let linknodes = future::try_join_all(linknodes).await?;
    for linknode in &linknodes {
        history_graph.insert(*linknode, None);
    }
    Ok(linknodes)
}

/// Returns history for a given unode if it exists.
async fn prefetch_history(
    ctx: &CoreContext,
    repo: &BlobRepo,
    unode_entry: UnodeEntry,
) -> Result<Option<Vec<(ChangesetId, Vec<FastlogParent>)>>, Error> {
    let blobstore: Arc<dyn Blobstore> = Arc::new(repo.get_blobstore());
    let maybe_fastlog_batch = fetch_fastlog_batch_by_unode_id(ctx, &blobstore, unode_entry).await?;
    if let Some(fastlog_batch) = maybe_fastlog_batch {
        let res = fetch_flattened(&fastlog_batch, ctx.clone(), blobstore)
            .compat()
            .await?;
        Ok(Some(res))
    } else {
        Ok(None)
    }
}

async fn derive_unode_entry(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: ChangesetId,
    path: &Option<MPath>,
) -> Result<Option<UnodeEntry>, Error> {
    let root_unode_mf_id = RootUnodeManifestId::derive(ctx.clone(), repo.clone(), cs_id)
        .compat()
        .await?;
    root_unode_mf_id
        .manifest_unode_id()
        .find_entry(ctx.clone(), repo.get_blobstore(), path.clone())
        .compat()
        .await
}

type CommitGraph = HashMap<ChangesetId, Option<Vec<ChangesetId>>>;

struct TraversalState<V: Visitor> {
    history_graph: CommitGraph,
    visited: HashSet<ChangesetId>,
    bfs: VecDeque<ChangesetId>,
    prefetch: Option<ChangesetId>,
    visitor: V,
}

async fn do_history_unfold<V>(
    ctx: CoreContext,
    repo: BlobRepo,
    path: Option<MPath>,
    state: TraversalState<V>,
    history_across_deletions: HistoryAcrossDeletions,
) -> Result<Option<(Vec<ChangesetId>, TraversalState<V>)>, Error>
where
    V: Visitor,
{
    let TraversalState {
        mut history_graph,
        mut visited,
        mut bfs,
        prefetch,
        mut visitor,
    } = state;

    if let Some(prefetch) = prefetch {
        prefetch_and_process_history(
            &ctx,
            &repo,
            &mut visitor,
            &path,
            prefetch.clone(),
            &mut history_graph,
        )
        .await?;
    }

    let mut history = vec![];
    // process nodes to yield
    let mut next_to_fetch = None;
    while let Some(cs_id) = bfs.pop_front() {
        match history_graph.get(&cs_id) {
            Some(Some(parents)) => {
                // parents are fetched, ready to process
                let ancestors = if parents.is_empty()
                    && history_across_deletions == HistoryAcrossDeletions::Track
                {
                    let (stats, deletion_nodes) =
                        find_where_file_was_deleted(&ctx, &repo, cs_id, &path)
                            .timed()
                            .await;
                    STATS::find_where_file_was_deleted_ms
                        .add_value(stats.completion_time.as_millis_unchecked() as i64);
                    let deletion_nodes = deletion_nodes?;
                    process_deletion_nodes(&ctx, &repo, &mut history_graph, deletion_nodes).await?
                } else {
                    parents.clone()
                };

                visit(
                    &ctx,
                    &repo,
                    &mut visitor,
                    Some(cs_id),
                    ancestors,
                    &mut bfs,
                    &mut visited,
                    &mut history,
                )
                .await?;
            }
            Some(None) => {
                // parents haven't been fetched yet
                // we want to proceed to next iteration to fetch the parents
                if Some(cs_id) == prefetch {
                    return Err(format_err!(
                        "internal error: infinite loop while traversing history for {:?}",
                        path
                    ));
                }
                next_to_fetch = Some(cs_id);
                // Put it back in the queue so we can process once we fetch its history
                bfs.push_front(cs_id);
                break;
            }
            // this should never happen as the [cs -> parents] mapping is fetched
            // from the fastlog batch. and if some cs id is mentioned as a parent
            // in the batch, the same batch has to have a record for this cs id.
            None => {}
        }
    }

    // Terminate when there's nothing to return and nothing on BFS queue.
    if history.is_empty() && bfs.is_empty() {
        return Ok(None);
    }
    Ok(Some((
        history,
        TraversalState {
            history_graph,
            visited,
            bfs,
            prefetch: next_to_fetch,
            visitor,
        },
    )))
}

// Now let's process commits which have a "path" in their manifests but
// their parent commits do not. That might mean one of two things:
// 1) a `path` was introduced in this commit and never existed before
// 2) a `path` was introduced in an ancestor of this commit, then deleted
//    and then re-introduced
//
// Case #2 needs a special processing - we need to check deleted file
// manifest of a parent commit and see if a commit was ever deleted.
async fn find_where_file_was_deleted(
    ctx: &CoreContext,
    repo: &BlobRepo,
    commit_no_more_history: ChangesetId,
    path: &Option<MPath>,
) -> Result<Vec<(ChangesetId, UnodeEntry)>, Error> {
    let parents = repo
        .get_changeset_parents_by_bonsai(ctx.clone(), commit_no_more_history)
        .compat()
        .await?;

    let resolved_path_states = future::try_join_all(
        parents
            .into_iter()
            .map(|p| resolve_path_state(ctx, repo, p, path)),
    )
    .await?;

    let mut all_deletion_nodes = vec![];
    for maybe_resolved_path_state in resolved_path_states {
        if let Some(resolved_path_states) = maybe_resolved_path_state {
            match resolved_path_states {
                PathState::Exists(_) => {
                    // shouldn't really happen in practice - if a parent has a unode
                    // then children should have a pointer to this unode
                    STATS::unexpected_existing_unode.add_value(1);
                }
                PathState::Deleted(deletion_nodes) => {
                    all_deletion_nodes.extend(deletion_nodes);
                }
            }
        }
    }

    Ok(all_deletion_nodes)
}

/// prefetches and processes fastlog batch for the given changeset id
async fn prefetch_and_process_history(
    ctx: &CoreContext,
    repo: &BlobRepo,
    visitor: &mut impl Visitor,
    path: &Option<MPath>,
    changeset_id: ChangesetId,
    history_graph: &mut CommitGraph,
) -> Result<(), Error> {
    let fastlog_batch = prefetch_fastlog_by_changeset(ctx, repo, changeset_id, path).await?;
    let affected_changesets: Vec<_> = fastlog_batch.iter().map(|(cs_id, _)| *cs_id).collect();
    process_unode_batch(fastlog_batch, history_graph);

    visitor
        .preprocess(
            ctx,
            repo,
            affected_changesets
                .into_iter()
                .filter_map(|cs_id| {
                    history_graph
                        .get(&cs_id)
                        .cloned()
                        .flatten()
                        .map(|parents| (Some(cs_id), parents))
                })
                .collect(),
        )
        .await?;
    Ok(())
}

fn process_unode_batch(
    unode_batch: Vec<(ChangesetId, Vec<FastlogParent>)>,
    graph: &mut CommitGraph,
) {
    for (cs_id, parents) in unode_batch {
        let has_unknown_parent = parents.iter().any(|parent| match parent {
            FastlogParent::Unknown => true,
            _ => false,
        });
        let known_parents: Vec<ChangesetId> = parents
            .into_iter()
            .filter_map(|parent| match parent {
                FastlogParent::Known(cs_id) => Some(cs_id),
                _ => None,
            })
            .collect();

        if let Some(maybe_parents) = graph.get(&cs_id) {
            // history graph has the changeset
            if maybe_parents.is_none() && !has_unknown_parent {
                // the node was visited but had unknown parents
                // let's update the graph
                graph.insert(cs_id, Some(known_parents.clone()));
            }
        } else {
            // we haven't seen this changeset before
            if has_unknown_parent {
                // at least one parent is unknown ->
                // need to fetch unode batch for this changeset
                //
                // let's add to the graph with None parents, this way we mark the
                // changeset as visited for other traversal branches
                graph.insert(cs_id, None);
            } else {
                graph.insert(cs_id, Some(known_parents.clone()));
            }
        }
    }
}

async fn prefetch_fastlog_by_changeset(
    ctx: &CoreContext,
    repo: &BlobRepo,
    changeset_id: ChangesetId,
    path: &Option<MPath>,
) -> Result<Vec<(ChangesetId, Vec<FastlogParent>)>, Error> {
    let unode_entry_opt = derive_unode_entry(ctx, repo, changeset_id.clone(), path).await?;
    let entry = unode_entry_opt
        .ok_or_else(|| format_err!("Unode entry is not found {:?} {:?}", changeset_id, path))?;

    // optimistically try to fetch history for a unode
    let fastlog_batch_opt = prefetch_history(ctx, repo, entry.clone()).await?;
    if let Some(batch) = fastlog_batch_opt {
        return Ok(batch);
    }

    // if there is no history, let's try to derive batched fastlog data
    // and fetch history again
    RootFastlog::derive(ctx.clone(), repo.clone(), changeset_id.clone())
        .compat()
        .await?;
    let fastlog_batch_opt = prefetch_history(ctx, repo, entry).await?;
    fastlog_batch_opt
        .ok_or_else(|| format_err!("Fastlog data is not found {:?} {:?}", changeset_id, path))
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::mapping::RootFastlog;
    use blobrepo_factory::new_memblob_empty;
    use context::CoreContext;
    use fbinit::FacebookInit;
    use futures::future::TryFutureExt;
    use tests_utils::CreateCommitContext;

    #[fbinit::compat_test]
    async fn test_list_linear_history(fb: FacebookInit) -> Result<(), Error> {
        // generate couple of hundreds linear file changes and list history
        let repo = new_memblob_empty(None).unwrap();
        let ctx = CoreContext::test_mock(fb);

        let filename = "1";

        let mut parents = vec![];
        let mut expected = vec![];
        for i in 1..300 {
            let file = if i % 2 == 1 { "2" } else { filename };
            let content = format!("{}", i);

            let bcs_id = CreateCommitContext::new(&ctx, &repo, parents)
                .add_file(file, content)
                .commit()
                .await?;
            if i % 2 != 1 {
                expected.push(bcs_id.clone());
            }
            parents = vec![bcs_id];
        }

        let top = parents.get(0).unwrap().clone();

        RootFastlog::derive(ctx.clone(), repo.clone(), top.clone())
            .compat()
            .await?;

        let history = list_file_history(
            ctx,
            repo,
            path(filename),
            top,
            (),
            HistoryAcrossDeletions::Track,
        )
        .await?;
        let history = history.try_collect::<Vec<_>>().await?;

        expected.reverse();
        assert_eq!(history, expected);
        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_list_history_with_merges(fb: FacebookInit) -> Result<(), Error> {
        // test generates commit graph with merges and compares result of list_file_history with
        // the result of BFS sorting on the graph
        //
        //           o - top
        //           |
        //           o - L+M
        //         / |
        //        o  o - L, M
        //         \ |
        //           o
        //           |
        //           :
        //           |
        //           o - A+B+C+D
        //           | \
        //     A+B - o  o
        //         / |  |
        //        o  o  o - C+D
        //        B  |  | \
        //           o  o  o
        //           |  |  |
        //           o  o  o
        //           |  C  D
        //           o
        //           A
        //

        let repo = new_memblob_empty(None).unwrap();
        let ctx = CoreContext::test_mock(fb);

        let filename = "1";
        let graph = HashMap::new();
        let branch_head = |branch, number, parents, graph| {
            create_branch(&ctx, &repo, branch, number, false, parents, graph)
                .map_ok(|(commits, graph)| (commits.last().unwrap().clone(), graph))
        };

        let (a_top, graph) = branch_head("A", 4, vec![], graph).await?;
        let (b_top, graph) = branch_head("B", 1, vec![], graph).await?;
        let (ab_top, graph) = branch_head("AB", 1, vec![a_top, b_top], graph).await?;

        let (c_top, graph) = branch_head("C", 2, vec![], graph).await?;
        let (d_top, graph) = branch_head("D", 2, vec![], graph).await?;
        let (cd_top, graph) = branch_head("CD", 2, vec![c_top, d_top], graph).await?;

        let (all_top, graph) = branch_head("ABCD", 105, vec![ab_top, cd_top], graph).await?;

        let (l_top, graph) = branch_head("L", 1, vec![all_top.clone()], graph).await?;
        let (m_top, graph) = branch_head("M", 1, vec![all_top.clone()], graph).await?;
        let (top, graph) = branch_head("Top", 2, vec![l_top, m_top], graph).await?;

        RootFastlog::derive(ctx.clone(), repo.clone(), top.clone())
            .compat()
            .await?;

        let history = list_file_history(
            ctx,
            repo,
            path(filename),
            top,
            (),
            HistoryAcrossDeletions::Track,
        )
        .await?;
        let history = history.try_collect::<Vec<_>>().await?;

        let expected = bfs(&graph, top);
        assert_eq!(history, expected);

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_list_history_many_diamonds(fb: FacebookInit) -> Result<(), Error> {
        // test generates commit graph with 50 diamonds
        //
        //              o - top
        //            /  \
        //           o    o
        //            \  /
        //             o
        //             |
        //             :
        //             |
        //             o
        //           /  \
        //          o    o
        //           \  /
        //            o
        //            |
        //            o - up
        //          /  \
        //  left - o    o - right
        //          \  /
        //           o - bottom
        //           |
        //           o
        //

        let repo = new_memblob_empty(None).unwrap();
        let ctx = CoreContext::test_mock(fb);

        let filename = "1";
        let mut expected = vec![];

        let root_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file(filename, "root")
            .commit()
            .await?;
        expected.push(root_id.clone());

        let mut prev_id = root_id;
        for _ in 0..50 {
            prev_id = create_diamond(&ctx, &repo, vec![prev_id], &mut expected).await?;
        }

        RootFastlog::derive(ctx.clone(), repo.clone(), prev_id.clone())
            .compat()
            .await?;

        let history = list_file_history(
            ctx,
            repo,
            path(filename),
            prev_id,
            (),
            HistoryAcrossDeletions::Track,
        )
        .await?;
        let history = history.try_collect::<Vec<_>>().await?;

        expected.reverse();
        assert_eq!(history, expected);

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_list_history_visitor(fb: FacebookInit) -> Result<(), Error> {
        // Test history termination on one of the history branches.
        // The main branch (top) and branch A have commits that change only single file.
        //
        // The history is long enough so it needs to prefetch fastlog batch for both A and B
        // branches.
        //
        //          o - top
        //          |
        //          o
        //          :
        //          o
        //         / \--------- we want to terminate this branch
        //    A - o   o - B
        //        |   |
        //        o   o
        //        :   :
        //        o   o
        //        |   |
        //        o   o
        //
        let repo = new_memblob_empty(None).unwrap();
        let ctx = CoreContext::test_mock(fb);

        let filename = "1";
        let filepath = path(filename);

        let graph = HashMap::new();

        let (mut a_branch, graph) =
            create_branch(&ctx, &repo, "A", 20, false, vec![], graph).await?;
        let a_top = a_branch.last().unwrap().clone();

        let (b_branch, graph) = create_branch(&ctx, &repo, "B", 20, true, vec![], graph).await?;
        let b_top = *b_branch.last().unwrap();

        let (mut main_branch, _graph) =
            create_branch(&ctx, &repo, "top", 100, false, vec![a_top, b_top], graph).await?;
        let top = *main_branch.last().unwrap();
        main_branch.reverse();

        struct NothingVisitor;
        #[async_trait]
        impl Visitor for NothingVisitor {
            async fn visit(
                &mut self,
                _ctx: &CoreContext,
                _repo: &BlobRepo,
                _descendant_cs_id: Option<ChangesetId>,
                _cs_ids: Vec<ChangesetId>,
            ) -> Result<Vec<ChangesetId>, Error> {
                Ok(vec![])
            }
        };
        let history = list_file_history(
            ctx.clone(),
            repo.clone(),
            filepath.clone(),
            top.clone(),
            NothingVisitor {},
            HistoryAcrossDeletions::Track,
        )
        .await?;
        let history = history.try_collect::<Vec<_>>().await?;

        // history now should be empty - the visitor prevented traversal
        assert_eq!(history, vec![]);

        // prune right branch
        struct SingleBranchOfHistoryVisitor;
        #[async_trait]
        impl Visitor for SingleBranchOfHistoryVisitor {
            async fn visit(
                &mut self,
                _ctx: &CoreContext,
                _repo: &BlobRepo,
                _descendant_cs_id: Option<ChangesetId>,
                cs_ids: Vec<ChangesetId>,
            ) -> Result<Vec<ChangesetId>, Error> {
                Ok(cs_ids.into_iter().next().into_iter().collect())
            }
        };
        let history = list_file_history(
            ctx,
            repo,
            filepath,
            top,
            SingleBranchOfHistoryVisitor {},
            HistoryAcrossDeletions::Track,
        )
        .await?;
        let history = history.try_collect::<Vec<_>>().await?;

        // the beginning of the history should be same as main branch
        // main_branch.reverse();
        assert_eq!(history[..100], main_branch[..100]);

        // the second part should be just a_branch
        a_branch.reverse();
        assert_eq!(history[100..], a_branch[..]);

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_list_history_deleted(fb: FacebookInit) -> Result<(), Error> {
        let repo = new_memblob_empty(None).unwrap();
        let ctx = CoreContext::test_mock(fb);

        let filename = "dir/1";
        let mut expected = vec![];

        let bcs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file(filename, "blah")
            .commit()
            .await?;
        expected.push(bcs_id.clone());
        let bcs_id = CreateCommitContext::new(&ctx, &repo, vec![bcs_id])
            .add_file("other_file", "1")
            .commit()
            .await?;

        let bcs_id = CreateCommitContext::new(&ctx, &repo, vec![bcs_id])
            .add_file(filename, "blah-blah")
            .commit()
            .await?;
        expected.push(bcs_id.clone());
        let bcs_id = CreateCommitContext::new(&ctx, &repo, vec![bcs_id])
            .add_file("other_file", "1-2")
            .commit()
            .await?;

        let bcs_id = CreateCommitContext::new(&ctx, &repo, vec![bcs_id])
            .delete_file(filename)
            .commit()
            .await?;
        expected.push(bcs_id.clone());
        let bcs_id = CreateCommitContext::new(&ctx, &repo, vec![bcs_id])
            .add_file("other_file", "1-2-3")
            .commit()
            .await?;

        let history = |cs_id, path| {
            cloned!(ctx, repo);
            async move {
                let history_stream = list_file_history(
                    ctx.clone(),
                    repo.clone(),
                    path,
                    cs_id,
                    (),
                    HistoryAcrossDeletions::Track,
                )
                .await?;
                history_stream.try_collect::<Vec<_>>().await
            }
        };

        expected.reverse();
        // check deleted file
        assert_eq!(history(bcs_id.clone(), path(filename)).await?, expected);
        // check deleted directory
        assert_eq!(history(bcs_id.clone(), path("dir")).await?, expected);

        // recreate dir and check
        let bcs_id = CreateCommitContext::new(&ctx, &repo, vec![bcs_id])
            .add_file("dir/otherfile", "boo")
            .commit()
            .await?;

        let mut res = vec![bcs_id];
        res.extend(expected);
        assert_eq!(history(bcs_id.clone(), path("dir")).await?, res);

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_list_history_merged_deleted(fb: FacebookInit) -> Result<(), Error> {
        //
        //     L
        //     |
        //     K
        //     | \
        //     J  H
        //     |  |
        //     I  G
        //     |  | \
        //     C  D  F
        //     | /   |
        //     B     E
        //     |
        //     A
        //
        let repo = new_memblob_empty(None).unwrap();
        let ctx = CoreContext::test_mock(fb);

        let a = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file", "1")
            .commit()
            .await?;

        let b = CreateCommitContext::new(&ctx, &repo, vec![a.clone()])
            .add_file("file", "1->2")
            .add_file("dir_1/file_1", "sub file 1")
            .add_file("dir_1/file_2", "sub file 2")
            .commit()
            .await?;

        let c = CreateCommitContext::new(&ctx, &repo, vec![b.clone()])
            .add_file("file", "1->2->3")
            .add_file("dir/file", "a")
            .add_file("dir_1/file_1", "sub file 1 amend")
            .commit()
            .await?;

        let d = CreateCommitContext::new(&ctx, &repo, vec![b.clone()])
            .delete_file("file")
            .add_file("dir/file", "b")
            .add_file("dir_1/file_2", "sub file 2 amend")
            .commit()
            .await?;

        let e = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file", "another 1")
            .commit()
            .await?;

        let f = CreateCommitContext::new(&ctx, &repo, vec![e.clone()])
            .add_file("file", "another 1 -> 2")
            .commit()
            .await?;

        let g = CreateCommitContext::new(&ctx, &repo, vec![d.clone(), f.clone()])
            .delete_file("file")
            .delete_file("dir/file")
            .delete_file("dir_1/file_2")
            .commit()
            .await?;

        let h = CreateCommitContext::new(&ctx, &repo, vec![g.clone()])
            .add_file("file-2", "smth")
            .commit()
            .await?;

        let i = CreateCommitContext::new(&ctx, &repo, vec![c.clone()])
            .delete_file("file")
            .delete_file("dir/file")
            .delete_file("dir_1/file_1")
            .commit()
            .await?;

        let j = CreateCommitContext::new(&ctx, &repo, vec![i.clone()])
            .add_file("file-3", "smth-2")
            .commit()
            .await?;

        let k = CreateCommitContext::new(&ctx, &repo, vec![j.clone(), h.clone()])
            .delete_file("file")
            .delete_file("dir_1/file_1")
            .delete_file("dir_1/file_2")
            .commit()
            .await?;

        let l = CreateCommitContext::new(&ctx, &repo, vec![k.clone()])
            .add_file("file-4", "smth-3")
            .commit()
            .await?;

        let history = |cs_id, path| {
            cloned!(ctx, repo);
            async move {
                let history_stream = list_file_history(
                    ctx.clone(),
                    repo.clone(),
                    path,
                    cs_id,
                    (),
                    HistoryAcrossDeletions::Track,
                )
                .await?;
                history_stream.try_collect::<Vec<_>>().await
            }
        };

        let expected = vec![
            i.clone(),
            g.clone(),
            d.clone(),
            c.clone(),
            f.clone(),
            b.clone(),
            e.clone(),
            a.clone(),
        ];
        assert_eq!(history(l.clone(), path("file")).await?, expected);

        let expected = vec![i.clone(), g.clone(), c.clone(), d.clone()];
        assert_eq!(history(l.clone(), path("dir/file")).await?, expected);

        let expected = vec![k.clone(), i.clone(), b.clone(), c.clone()];
        assert_eq!(history(l.clone(), path("dir_1/file_1")).await?, expected);

        let expected = vec![
            k.clone(),
            i.clone(),
            g.clone(),
            c.clone(),
            d.clone(),
            b.clone(),
        ];
        assert_eq!(history(l.clone(), path("dir_1")).await?, expected);

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_list_history_across_deletions_linear(fb: FacebookInit) -> Result<(), Error> {
        let repo = new_memblob_empty(None).unwrap();
        let ctx = CoreContext::test_mock(fb);

        let filename = "dir/1";
        let mut expected = vec![];

        let bcs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file(filename, "content1")
            .commit()
            .await?;
        expected.push(bcs_id.clone());
        let bcs_id = CreateCommitContext::new(&ctx, &repo, vec![bcs_id])
            .delete_file(filename)
            .commit()
            .await?;
        expected.push(bcs_id.clone());
        let bcs_id = CreateCommitContext::new(&ctx, &repo, vec![bcs_id])
            .add_file(filename, "content2")
            .commit()
            .await?;
        expected.push(bcs_id.clone());

        let history_stream = list_file_history(
            ctx.clone(),
            repo.clone(),
            MPath::new_opt(filename)?,
            bcs_id,
            (),
            HistoryAcrossDeletions::Track,
        )
        .await?;
        let expected = expected.into_iter().rev().collect::<Vec<_>>();

        let actual = history_stream.try_collect::<Vec<_>>().await?;
        assert_eq!(actual, expected);

        let history_stream = list_file_history(
            ctx.clone(),
            repo.clone(),
            MPath::new_opt(filename)?,
            bcs_id,
            (),
            HistoryAcrossDeletions::DontTrack,
        )
        .await?;
        let actual = history_stream.try_collect::<Vec<_>>().await?;
        assert_eq!(actual, vec![bcs_id]);

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_list_history_across_deletions_with_merges(fb: FacebookInit) -> Result<(), Error> {
        let repo = new_memblob_empty(None).unwrap();
        let ctx = CoreContext::test_mock(fb);

        let filename = "dir/1";
        let mut expected = vec![];

        let bcs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file(filename, "content1")
            .commit()
            .await?;
        expected.push(bcs_id.clone());
        let bcs_id = CreateCommitContext::new(&ctx, &repo, vec![bcs_id])
            .delete_file(filename)
            .commit()
            .await?;
        expected.push(bcs_id.clone());

        let bcs_p1 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id])
            .add_file("p1file", "p1")
            .commit()
            .await?;
        let bcs_p2 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id])
            .add_file("p2file", "p2")
            .commit()
            .await?;

        let merge = CreateCommitContext::new(&ctx, &repo, vec![bcs_p1, bcs_p2])
            .add_file("mergefile", "merge")
            .commit()
            .await?;
        let bcs_id = CreateCommitContext::new(&ctx, &repo, vec![merge])
            .add_file(filename, "aftermerge")
            .commit()
            .await?;
        expected.push(bcs_id);

        //    O <- recreates "dir/1"
        //    |
        //    O
        //   /  \
        //  O    0
        //   \  /
        //    0 <- removes "dir/1"
        //    |
        //    0  <- creates "dir/1"

        let history_stream = list_file_history(
            ctx.clone(),
            repo.clone(),
            MPath::new_opt(filename)?,
            bcs_id,
            (),
            HistoryAcrossDeletions::Track,
        )
        .await?;
        let mut expected = expected.into_iter().rev().collect::<Vec<_>>();

        let actual = history_stream.try_collect::<Vec<_>>().await?;
        assert_eq!(actual, expected);

        // Now check the history starting from a merge commit
        let history_stream = list_file_history(
            ctx.clone(),
            repo.clone(),
            MPath::new_opt(filename)?,
            merge,
            (),
            HistoryAcrossDeletions::Track,
        )
        .await?;
        expected.remove(0);

        let actual = history_stream.try_collect::<Vec<_>>().await?;
        assert_eq!(actual, expected);
        Ok(())
    }

    type TestCommitGraph = HashMap<ChangesetId, Vec<ChangesetId>>;

    async fn create_branch(
        ctx: &CoreContext,
        repo: &BlobRepo,
        branch: &str,
        number: i32,
        // add one more file change for each commit in the branch
        branch_file: bool,
        mut parents: Vec<ChangesetId>,
        mut graph: TestCommitGraph,
    ) -> Result<(Vec<ChangesetId>, TestCommitGraph), Error> {
        let filename = "1";
        let mut commits = vec![];
        for i in 0..number {
            let mut bcs = CreateCommitContext::new(ctx, repo, parents.clone())
                .add_file(filename, format!("{} - {}", branch, i));
            if branch_file {
                bcs = bcs.add_file(branch, format!("{}", i));
            }
            let bcs_id = bcs.commit().await?;

            graph.insert(bcs_id.clone(), parents);
            commits.push(bcs_id);
            parents = vec![bcs_id];
        }
        Ok((commits, graph))
    }

    async fn create_diamond(
        ctx: &CoreContext,
        repo: &BlobRepo,
        parents: Vec<ChangesetId>,
        expected: &mut Vec<ChangesetId>,
    ) -> Result<ChangesetId, Error> {
        let filename = "1";
        // bottom
        let bottom_id = CreateCommitContext::new(ctx, repo, parents.clone())
            .add_file(filename, format!("B - {:?}", parents))
            .commit()
            .await?;
        expected.push(bottom_id.clone());

        // right
        let right_id = CreateCommitContext::new(ctx, repo, vec![bottom_id])
            .add_file(filename, format!("R - {:?}", parents))
            .commit()
            .await?;
        expected.push(right_id.clone());

        // left
        let left_id = CreateCommitContext::new(ctx, repo, vec![bottom_id])
            .add_file(filename, format!("L - {:?}", parents))
            .commit()
            .await?;
        expected.push(left_id.clone());

        // up
        let up_id = CreateCommitContext::new(ctx, repo, vec![left_id, right_id])
            .add_file(filename, format!("U - {:?}", parents))
            .commit()
            .await?;
        expected.push(up_id.clone());

        Ok(up_id)
    }

    fn bfs(graph: &TestCommitGraph, node: ChangesetId) -> Vec<ChangesetId> {
        let mut response = vec![];
        let mut queue = VecDeque::new();
        let mut visited = HashSet::new();
        queue.push_back(node.clone());
        visited.insert(node);

        while let Some(node) = queue.pop_front() {
            if let Some(parents) = graph.get(&node) {
                for p in parents {
                    if visited.insert(*p) {
                        queue.push_back(*p);
                    }
                }
            }
            response.push(node);
        }
        response
    }

    fn path(path_str: &str) -> Option<MPath> {
        MPath::new(path_str).ok()
    }
}
