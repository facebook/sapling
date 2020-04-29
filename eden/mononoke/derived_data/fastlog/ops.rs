/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Error};
use blobrepo::BlobRepo;
use blobstore::{Blobstore, Loadable, LoadableError};
use bounded_traversal::bounded_traversal_stream;
use cloned::cloned;
use context::CoreContext;
use deleted_files_manifest::{self as deleted_manifest, RootDeletedManifestId};
use derived_data::{BonsaiDerived, DeriveError};
use futures::{
    compat::Future01CompatExt,
    future::{self, FutureExt as NewFutureExt, TryFutureExt},
    stream::{self, Stream as NewStream},
};
use futures_old::Future;
use futures_util::{StreamExt, TryStreamExt};
use manifest::{Entry, ManifestOps};
use maplit::hashset;
use mononoke_types::{ChangesetId, FileUnodeId, MPath, ManifestUnodeId};
use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future as NewFuture;
use std::iter::FromIterator;
use std::sync::Arc;
use thiserror::Error;
use unodes::RootUnodeManifestId;

use crate::fastlog_impl::{fetch_fastlog_batch_by_unode_id, fetch_flattened};
use crate::mapping::{FastlogParent, RootFastlog};

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

/// Returns a full history of the given path starting from the given unode in BFS order.
///
/// Can accept a terminator function: a function on changeset id, that returns true if
/// the history fetching on the current branch has to be terminated.
/// The terminator will be called on changeset id when a fatslog batch is going to be
/// fetched for the changeset. If the terminator returns true, fastlog is not fetched,
/// which means that this history branch is terminated. Already prefetched commits are
/// still streamed.
/// It is possible that of history is not linear and have 2 or more branches, terminator
/// can drop history fetching on one of the branches and still proceed with others.
/// Usage:
///       as history stream generally is not ordered by commit creation time (due to
///       the BFS order), it's still necessary to drop the stream if the history is
///       already older than the given time frame.
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
/// On each step of bounded_traversal_stream:
///   1 - prefetch fastlog batch for the `prefetch` changeset id and fill the commit graph
///   2 - perform BFS until the node for which parents haven't been prefetched
///   3 - stream all the "ready" nodes and set the last node to prefetch
/// The stream stops when there is nothing to return.
///
/// Why to pop all nodes on the same depth and not just one commit at a time?
/// Because if history contains merges and parents for more than one node on the current depth
/// haven't been fetched yet, we can fetch them at the same time using FuturesUnordered.
pub async fn list_file_history<Terminator, TFut>(
    ctx: CoreContext,
    repo: BlobRepo,
    path: Option<MPath>,
    changeset_id: ChangesetId,
    terminator: Option<Terminator>,
) -> Result<impl NewStream<Item = Result<ChangesetId, Error>>, FastlogError>
where
    Terminator: Fn(ChangesetId) -> TFut + 'static + Clone + Send + Sync,
    TFut: NewFuture<Output = Result<bool, Error>> + Send,
{
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
    // there might be more than one unode entry: if the given path was
    // deleted in several different branches, we have to gather history
    // from all of them
    let unode_entries = match resolved {
        Some(PathState::Deleted(deletion_nodes)) => {
            // we want to show commit, where path was deleted
            let mut entries = vec![];
            let mut visited = HashSet::new();
            for (deleted_linknode, last_unode_entry) in deletion_nodes {
                if visited.insert(deleted_linknode.clone()) {
                    // there might be one linknode for the several entries
                    // for example, if the path was deleted in merge commit
                    top_history.push(Ok(deleted_linknode));
                }
                entries.push(last_unode_entry);
            }
            entries
        }
        Some(PathState::Exists(unode_entry)) => vec![unode_entry],
        None => {
            return Err(not_found_err());
        }
    };

    let last_changesets = unode_entries.into_iter().map({
        cloned!(ctx, repo);
        move |entry| {
            cloned!(ctx, repo);
            async move {
                let unode = entry
                    .load(ctx.clone(), &repo.get_blobstore())
                    .compat()
                    .await?;
                Ok::<_, FastlogError>(match unode {
                    Entry::Tree(mf_unode) => mf_unode.linknode().clone(),
                    Entry::Leaf(file_unode) => file_unode.linknode().clone(),
                })
            }
        }
    });
    let last_changesets = future::try_join_all(last_changesets).await?;

    let history_graph =
        HashMap::from_iter(last_changesets.clone().into_iter().map(|cs| (cs, None)));
    let visited = HashSet::from_iter(last_changesets.clone().into_iter());

    let mut last_changesets = last_changesets.into_iter();
    let the_last_change = last_changesets.next().ok_or_else(not_found_err)?;
    top_history.push(Ok(the_last_change.clone()));
    let bfs = VecDeque::from_iter(last_changesets);

    // generate file history
    Ok(stream::iter(top_history)
        .chain({
            bounded_traversal_stream(
                256,
                // starting point
                Some(TraversalState {
                    history_graph,
                    visited,
                    bfs,
                    prefetch: the_last_change,
                }),
                // unfold
                move |state| {
                    cloned!(ctx, repo, path, terminator);
                    async move {
                        do_history_unfold(
                            ctx.clone(),
                            repo.clone(),
                            path.clone(),
                            state,
                            terminator,
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

/// Returns history for a given unode if it exists.
///
/// TODO(aida): This is no longer a public API, however APIServer still uses it.
/// Needs to be changed after APIServer will be deprecated.
pub fn prefetch_history(
    ctx: CoreContext,
    repo: BlobRepo,
    unode_entry: UnodeEntry,
) -> impl Future<Item = Option<Vec<(ChangesetId, Vec<FastlogParent>)>>, Error = Error> {
    let blobstore: Arc<dyn Blobstore> = Arc::new(repo.get_blobstore());
    async move {
        let maybe_fastlog_batch =
            fetch_fastlog_batch_by_unode_id(&ctx, &blobstore, unode_entry).await?;
        match maybe_fastlog_batch {
            Some(fastlog_batch) => {
                let res = fetch_flattened(&fastlog_batch, ctx, blobstore)
                    .compat()
                    .await?;
                Ok(Some(res))
            }
            None => Ok(None),
        }
    }
    .boxed()
    .compat()
}

type UnodeEntry = Entry<ManifestUnodeId, FileUnodeId>;

enum PathState {
    // changeset where the path was deleted and unode where the path was last changed
    Deleted(Vec<(ChangesetId, UnodeEntry)>),
    // unode if the path exists
    Exists(UnodeEntry),
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

async fn resolve_path_state(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: ChangesetId,
    path: &Option<MPath>,
) -> Result<Option<PathState>, FastlogError> {
    // if unode exists return entry
    let unode_entry = derive_unode_entry(ctx, repo, cs_id.clone(), path).await?;
    if let Some(unode_entry) = unode_entry {
        return Ok(Some(PathState::Exists(unode_entry)));
    }

    let use_deleted_manifest = repo
        .get_derived_data_config()
        .derived_data_types
        .contains(RootDeletedManifestId::NAME);
    if !use_deleted_manifest {
        return Ok(None);
    }

    // if there is no unode for the commit:path, check deleted manifest
    // the path might be deleted
    stream::try_unfold(
        // starting state
        (VecDeque::from(vec![cs_id.clone()]), hashset! { cs_id }),
        // unfold
        {
            cloned!(ctx, repo, path);
            move |(queue, visited)| {
                resolve_path_state_unfold(ctx.clone(), repo.clone(), path.clone(), queue, visited)
            }
        },
    )
    .map_ok(|deleted_nodes| stream::iter(deleted_nodes).map(Ok::<_, FastlogError>))
    .try_flatten()
    .try_collect::<Vec<_>>()
    .map_ok(move |deleted_nodes| {
        if deleted_nodes.is_empty() {
            None
        } else {
            Some(PathState::Deleted(deleted_nodes))
        }
    })
    .boxed()
    .await
}

async fn resolve_path_state_unfold(
    ctx: CoreContext,
    repo: BlobRepo,
    path: Option<MPath>,
    mut queue: VecDeque<ChangesetId>,
    mut visited: HashSet<ChangesetId>,
) -> Result<
    Option<(
        Vec<(ChangesetId, UnodeEntry)>,
        (VecDeque<ChangesetId>, HashSet<ChangesetId>),
    )>,
    FastlogError,
> {
    // let's get deleted manifests for each changeset id
    // and try to find the given path
    if let Some(cs_id) = queue.pop_front() {
        let root_dfm_id = RootDeletedManifestId::derive(ctx.clone(), repo.clone(), cs_id.clone())
            .compat()
            .await?;
        let dfm_id = root_dfm_id.deleted_manifest_id();
        let entry =
            deleted_manifest::find_entry(ctx.clone(), repo.get_blobstore(), *dfm_id, path.clone())
                .compat()
                .await?;

        if let Some(mf_id) = entry {
            // we need to get the linknode, so let's load the deleted manifest
            // if the linknodes is None it means that file should exist
            // but it doesn't, let's throw an error
            let mf = mf_id.load(ctx.clone(), repo.blobstore()).compat().await?;
            let linknode = mf.linknode().ok_or_else(|| {
                let message = format!(
                    "there is no unode for the path '{}' and changeset {:?}, but it exists as a live entry in deleted manifest",
                    MPath::display_opt(path.as_ref()),
                    cs_id,
                );
                FastlogError::InternalError(message)
            })?;

            // to get last change before deletion we have to look at the liknode
            // parents for the deleted path
            let parents = repo
                .get_changeset_parents_by_bonsai(ctx.clone(), linknode.clone())
                .compat()
                .await?;

            // checking parent unodes
            let parent_unodes = parents.into_iter().map({
                cloned!(ctx, repo, path);
                move |parent| {
                    cloned!(ctx, repo, path);
                    async move {
                        let unode_entry =
                            derive_unode_entry(&ctx, &repo, parent.clone(), &path).await?;
                        Ok::<_, FastlogError>((parent, unode_entry))
                    }
                }
            });
            let parent_unodes = future::try_join_all(parent_unodes).await?;
            return match *parent_unodes {
                [] => {
                    // the linknode must have a parent, otherwise the path couldn't be deleted
                    let message = format!(
                        "the path '{}' was deleted in {:?}, but the changeset doesn't have parents",
                        MPath::display_opt(path.as_ref()),
                        linknode,
                    );
                    Err(FastlogError::InternalError(message))
                }
                [(_parent, unode_entry)] => {
                    if let Some(unode_entry) = unode_entry {
                        // we've found the last path change before deletion
                        Ok(Some((vec![(linknode, unode_entry)], (queue, visited))))
                    } else {
                        // the unode entry must exist
                        let message = format!(
                            "the path '{}' was deleted in {:?}, but the parent changeset doesn't have a unode",
                            MPath::display_opt(path.as_ref()),
                            linknode,
                        );
                        Err(FastlogError::InternalError(message))
                    }
                }
                _ => {
                    let mut last_changes = vec![];
                    for (parent, unode_entry) in parent_unodes.into_iter() {
                        if let Some(unode_entry) = unode_entry {
                            // this is one of the last changes
                            last_changes.push((linknode, unode_entry));
                        } else {
                            // the path could have been already deleted here
                            // need to add this node into the queue
                            if visited.insert(parent.clone()) {
                                queue.push_back(parent);
                            }
                        }
                    }
                    Ok(Some((last_changes, (queue, visited))))
                }
            };
        }

        // the path was not deleted here, but could be deleted in other branches
        return Ok(Some((vec![], (queue, visited))));
    }

    Ok(None)
}

type CommitGraph = HashMap<ChangesetId, Option<Vec<ChangesetId>>>;

struct TraversalState {
    history_graph: CommitGraph,
    visited: HashSet<ChangesetId>,
    bfs: VecDeque<ChangesetId>,
    prefetch: ChangesetId,
}

async fn do_history_unfold<Terminator, TFut>(
    ctx: CoreContext,
    repo: BlobRepo,
    path: Option<MPath>,
    state: TraversalState,
    terminator: Option<Terminator>,
) -> Result<(Vec<ChangesetId>, Option<TraversalState>), Error>
where
    Terminator: Fn(ChangesetId) -> TFut + Clone,
    TFut: NewFuture<Output = Result<bool, Error>>,
{
    let TraversalState {
        history_graph,
        mut visited,
        mut bfs,
        prefetch,
    } = state;

    let terminate = match terminator {
        Some(terminator) => terminator(prefetch.clone()).await?,
        _ => false,
    };
    let history_graph = if !terminate {
        prefetch_and_process_history(&ctx, &repo, &path, prefetch.clone(), history_graph).await?
    } else {
        history_graph
    };

    // `prefetch` changeset is not in bfs queue anymore and neither it's parents
    // in order to traverse its parents we need to explicitly add them to the queue
    if let Some(Some(parents)) = history_graph.get(&prefetch) {
        // parents are fetched, ready to process
        for p in parents {
            if visited.insert(*p) {
                bfs.push_back(*p);
            }
        }
    }

    // process nodes to yield
    let mut next_to_fetch = None;
    let mut history = vec![];
    while let Some(cs_id) = bfs.pop_front() {
        history.push(cs_id.clone());
        match history_graph.get(&cs_id) {
            Some(Some(parents)) => {
                // parents are fetched, ready to process
                for p in parents {
                    if visited.insert(*p) {
                        bfs.push_back(*p);
                    }
                }
            }
            Some(None) => {
                // parents haven't been fetched yet
                // we want to proceed to next iteration to fetch the parents
                next_to_fetch = Some(cs_id);
                break;
            }
            // this should never happen as the [cs -> parents] mapping is fetched
            // from the fastlog batch. and if some cs id is mentioned as a parent
            // in the batch, the same batch has to have a record for this cs id.
            None => {}
        }
    }

    let new_state = if let Some(prefetch) = next_to_fetch {
        Some(TraversalState {
            history_graph,
            visited,
            bfs,
            prefetch,
        })
    } else {
        None
    };

    Ok((history, new_state))
}

/// prefetches and processes fastlog batch for the given changeset id
async fn prefetch_and_process_history(
    ctx: &CoreContext,
    repo: &BlobRepo,
    path: &Option<MPath>,
    changeset_id: ChangesetId,
    mut history_graph: CommitGraph,
) -> Result<CommitGraph, Error> {
    let fastlog_batch = prefetch_fastlog_by_changeset(ctx, repo, changeset_id, path).await?;
    process_unode_batch(fastlog_batch, &mut history_graph);
    Ok(history_graph)
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
    let fastlog_batch_opt = prefetch_history(ctx.clone(), repo.clone(), entry.clone())
        .compat()
        .await?;
    if let Some(batch) = fastlog_batch_opt {
        return Ok(batch);
    }

    // if there is no history, let's try to derive batched fastlog data
    // and fetch history again
    RootFastlog::derive(ctx.clone(), repo.clone(), changeset_id.clone())
        .compat()
        .await?;
    let fastlog_batch_opt = prefetch_history(ctx.clone(), repo.clone(), entry)
        .compat()
        .await?;
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
    use futures::future;
    use mononoke_types::{ChangesetId, MPath};
    use std::collections::{HashMap, HashSet, VecDeque};
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

        let terminator = |_cs_id| future::ready(Ok(false));
        let history = list_file_history(ctx, repo, path(filename), top, Some(terminator)).await?;
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

        let terminator = |_cs_id| future::ready(Ok(false));
        let history = list_file_history(ctx, repo, path(filename), top, Some(terminator)).await?;
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

        let terminator = |_cs_id| future::ready(Ok(false));
        let history =
            list_file_history(ctx, repo, path(filename), prev_id, Some(terminator)).await?;
        let history = history.try_collect::<Vec<_>>().await?;

        expected.reverse();
        assert_eq!(history, expected);

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_list_history_terminator(fb: FacebookInit) -> Result<(), Error> {
        // Test history termination on one of the history branches.
        // The main branch (top) and branch A have commits that change only single file.
        // Branch B changes 2 files and this is used as a termination condition.
        // The history is long enough so it needs to prefetch fastlog batch for both A and B
        // branches.
        //
        //          o - top   _
        //          |          |
        //          o          |
        //          :          |
        //          o          |- single fastlog batch
        //         / \         |
        //    A - o   o - B    |
        //        |   |        |
        //        o   o        |
        //        :   :       _| <- we want terminate here
        //        o   o          - other two fastlog batches
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

        let (main_branch, graph) =
            create_branch(&ctx, &repo, "top", 100, false, vec![a_top, b_top], graph).await?;
        let top = *main_branch.last().unwrap();

        // prune all fastlog batch fetchings
        let terminator = Some(|_cs_id| future::ready(Ok(true)));
        let history = list_file_history(
            ctx.clone(),
            repo.clone(),
            filepath.clone(),
            top.clone(),
            terminator,
        )
        .await?;
        let history = history.try_collect::<Vec<_>>().await?;

        // history now should represent only a single commit - the first one
        assert_eq!(history, vec![top.clone()]);

        // prune right branch on fastlog batch fetching
        let terminator = move |ctx: CoreContext, repo: BlobRepo, cs_id: ChangesetId| async move {
            let cs = cs_id.load(ctx.clone(), repo.blobstore()).compat().await?;
            let files = cs.file_changes_map();
            Ok(files.len() > 1)
        };
        let terminator = Some({
            cloned!(ctx, repo);
            move |cs_id| terminator(ctx.clone(), repo.clone(), cs_id)
        });
        let history = list_file_history(ctx, repo, filepath, top, terminator).await?;
        let history = history.try_collect::<Vec<_>>().await?;

        // the beginning of the history should be same as bfs
        let expected = bfs(&graph, top);
        assert_eq!(history[..109], expected[..109]);

        // last 15 commits of the history should be last 15 of the branch A
        a_branch.reverse();
        assert_eq!(history[109..], a_branch[5..]);

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
                let terminator = Some(|_cs_id| future::ready(Ok(false)));
                let history_stream =
                    list_file_history(ctx.clone(), repo.clone(), path, cs_id, terminator).await?;
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
        assert_eq!(history(bcs_id.clone(), path("dir")).await?, vec![bcs_id]);

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
                let terminator = Some(|_cs_id| future::ready(Ok(false)));
                let history_stream =
                    list_file_history(ctx.clone(), repo.clone(), path, cs_id, terminator).await?;
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
