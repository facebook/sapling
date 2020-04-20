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
    future::{FutureExt as NewFutureExt, TryFutureExt},
    stream::{self, Stream as NewStream},
};
use futures_old::Future;
use futures_util::{StreamExt, TryStreamExt};
use manifest::{Entry, ManifestOps};
use maplit::{hashmap, hashset};
use mononoke_types::{ChangesetId, FileUnodeId, MPath, ManifestUnodeId};
use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future as NewFuture;
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
    let resolved = resolve_path_state(&ctx, &repo, changeset_id, &path).await?;
    let unode_entry = match resolved {
        Some(PathState::Deleted(deleted_linknode, last_unode_entry)) => {
            // we want to show commit, where path was deleted
            top_history.push(Ok(deleted_linknode));
            last_unode_entry
        }
        Some(PathState::Exists(unode_entry)) => unode_entry,
        None => {
            return Err(if let Some(p) = path {
                FastlogError::NoSuchPath(p)
            } else {
                FastlogError::InternalError("cannot find unode for the repo root".to_string())
            });
        }
    };

    let unode = unode_entry
        .load(ctx.clone(), &repo.get_blobstore())
        .compat()
        .await?;

    let changeset_id = match unode {
        Entry::Tree(mf_unode) => mf_unode.linknode().clone(),
        Entry::Leaf(file_unode) => file_unode.linknode().clone(),
    };
    top_history.push(Ok(changeset_id.clone()));

    // generate file history
    Ok(stream::iter(top_history)
        .chain({
            let history_graph = hashmap! { changeset_id.clone() => None };
            let visited = hashset! { changeset_id.clone() };
            let bfs = VecDeque::new();

            bounded_traversal_stream(
                256,
                // starting point
                Some(TraversalState {
                    history_graph,
                    visited,
                    bfs,
                    prefetch: changeset_id,
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
    Deleted(ChangesetId, UnodeEntry),
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
    let root_dfm_id = RootDeletedManifestId::derive(ctx.clone(), repo.clone(), cs_id)
        .compat()
        .await?;
    let dfm_id = root_dfm_id.deleted_manifest_id();
    let deleted_entry =
        deleted_manifest::find_entry(ctx.clone(), repo.get_blobstore(), *dfm_id, path.clone())
            .compat()
            .await?;

    let internal_err = |p: &Option<MPath>| {
        let message = format!(
            "couldn't find '{}' last change before deletion",
            MPath::display_opt(p.as_ref())
        );
        Err(FastlogError::InternalError(message))
    };
    if let Some(deleted_entry) = deleted_entry {
        let deleted_node = deleted_entry
            .load(ctx.clone(), repo.blobstore())
            .compat()
            .await?;
        if let Some(linknode) = deleted_node.linknode() {
            // the path was deleted, let's find the last unode
            let parents = repo
                .get_changeset_parents_by_bonsai(ctx.clone(), linknode.clone())
                .compat()
                .await?;
            match *parents {
                [] => {
                    // the linknode must have a parent, otherwise the path couldn't be deleted
                    return internal_err(path);
                }
                [parent] => {
                    let unode_entry = derive_unode_entry(ctx, repo, parent, path).await?;
                    if let Some(unode_entry) = unode_entry {
                        // we've found the last path change before deletion
                        return Ok(Some(PathState::Deleted(*linknode, unode_entry)));
                    }

                    // the unode entry must exist
                    return internal_err(path);
                }
                [..] => {
                    // merged repos are not supported yet
                    return Ok(None);
                }
            }
        } else {
            // there is an entry in the deleted manifest but the path hasn't been deleted
            return Ok(None);
        }
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
    use blobrepo::save_bonsai_changesets;
    use blobrepo_factory::new_memblob_empty;
    use context::CoreContext;
    use fbinit::FacebookInit;
    use fixtures::{create_bonsai_changeset_with_files, store_files};
    use futures::future;
    use maplit::btreemap;
    use mononoke_types::{ChangesetId, MPath};
    use std::collections::{HashMap, HashSet, VecDeque};
    use tests_utils::CreateCommitContext;
    use tokio_compat::runtime::Runtime;

    #[fbinit::test]
    fn test_list_linear_history(fb: FacebookInit) {
        // generate couple of hundreds linear file changes and list history
        let repo = new_memblob_empty(None).unwrap();
        let mut rt = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock(fb);

        let filename = "1";
        let filepath = path(filename);

        let mut bonsais = vec![];
        let mut parents = vec![];
        let mut expected = vec![];
        for i in 1..300 {
            let file = if i % 2 == 1 { "2" } else { filename };
            let content = format!("{}", i);
            let stored_files = rt.block_on_std(store_files(
                ctx.clone(),
                btreemap! { file => Some(content.as_str()) },
                repo.clone(),
            ));

            let bcs = create_bonsai_changeset_with_files(parents, stored_files);
            let bcs_id = bcs.get_changeset_id();
            bonsais.push(bcs);
            if i % 2 != 1 {
                expected.push(bcs_id.clone());
            }
            parents = vec![bcs_id];
        }

        let latest = parents.get(0).unwrap().clone();
        rt.block_on(save_bonsai_changesets(bonsais, ctx.clone(), repo.clone()))
            .unwrap();

        derive_fastlog(ctx.clone(), repo.clone(), &mut rt, latest.clone());

        let terminator = |_cs_id| future::ready(Ok(false));
        let history = rt
            .block_on_std(list_file_history(
                ctx,
                repo,
                filepath,
                latest,
                Some(terminator),
            ))
            .unwrap();
        let history = rt.block_on_std(history.try_collect::<Vec<_>>()).unwrap();

        expected.reverse();
        assert_eq!(history, expected);
    }

    #[fbinit::test]
    fn test_list_history_with_merges(fb: FacebookInit) {
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
        let mut rt = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock(fb);

        let filename = "1";
        let filepath = path(filename);

        let mut bonsais = vec![];
        let mut graph = HashMap::new();
        let mut create_branch = |branch, number, mut parents: Vec<_>| {
            for i in 0..number {
                let content = format!("{} - {}", branch, i);
                let stored_files = rt.block_on_std(store_files(
                    ctx.clone(),
                    btreemap! { filename => Some(content.as_str()) },
                    repo.clone(),
                ));

                let bcs = create_bonsai_changeset_with_files(parents.clone(), stored_files);
                let bcs_id = bcs.get_changeset_id();
                bonsais.push(bcs);

                graph.insert(bcs_id.clone(), parents);
                parents = vec![bcs_id];
            }
            parents.get(0).unwrap().clone()
        };

        let a_top = create_branch("A", 4, vec![]);
        let b_top = create_branch("B", 1, vec![]);
        let ab_top = create_branch("A+B", 1, vec![a_top, b_top]);

        let c_top = create_branch("C", 2, vec![]);
        let d_top = create_branch("D", 2, vec![]);
        let cd_top = create_branch("C+D", 2, vec![c_top, d_top]);

        let all_top = create_branch("A+B+C+D", 105, vec![ab_top, cd_top]);

        let l_top = create_branch("L", 1, vec![all_top.clone()]);
        let m_top = create_branch("M", 1, vec![all_top.clone()]);
        let top = create_branch("Top", 2, vec![l_top, m_top]);

        rt.block_on(save_bonsai_changesets(bonsais, ctx.clone(), repo.clone()))
            .unwrap();

        derive_fastlog(ctx.clone(), repo.clone(), &mut rt, top.clone());

        let terminator = |_cs_id| future::ready(Ok(false));
        let history = rt
            .block_on_std(list_file_history(
                ctx,
                repo,
                filepath,
                top,
                Some(terminator),
            ))
            .unwrap();
        let history = rt.block_on_std(history.try_collect::<Vec<_>>()).unwrap();

        let expected = bfs(&graph, top);
        assert_eq!(history, expected);
    }

    #[fbinit::test]
    fn test_list_history_many_diamonds(fb: FacebookInit) {
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
        let mut rt = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock(fb);

        let filename = "1";
        let filepath = path(filename);

        let create_changeset = |content: String, parents: Vec<_>| {
            let ctx = &ctx;
            let repo = &repo;
            async move {
                let stored_files = store_files(
                    ctx.clone(),
                    btreemap! { filename => Some(content.as_str()) },
                    repo.clone(),
                )
                .await;

                create_bonsai_changeset_with_files(parents, stored_files)
            }
        };

        let mut bonsais = vec![];
        let mut expected = vec![];

        let root = rt.block_on_std(create_changeset("root".to_string(), vec![]));
        let root_id = root.get_changeset_id();
        bonsais.push(root);
        expected.push(root_id.clone());

        let mut create_diamond = |number, parents: Vec<_>| {
            // bottom
            let bcs = rt.block_on_std(create_changeset(format!("B - {}", number), parents.clone()));
            let bottom_id = bcs.get_changeset_id();
            bonsais.push(bcs);
            expected.push(bottom_id.clone());

            // right
            let bcs = rt.block_on_std(create_changeset(format!("R - {}", number), vec![bottom_id]));
            let right_id = bcs.get_changeset_id();
            bonsais.push(bcs);
            expected.push(right_id.clone());

            // left
            let bcs = rt.block_on_std(create_changeset(format!("L - {}", number), vec![bottom_id]));
            let left_id = bcs.get_changeset_id();
            bonsais.push(bcs);
            expected.push(left_id.clone());

            // up
            let bcs = rt.block_on_std(create_changeset(
                format!("U - {}", number),
                vec![left_id, right_id],
            ));
            let up_id = bcs.get_changeset_id();
            bonsais.push(bcs);
            expected.push(up_id.clone());

            up_id
        };

        let mut prev_id = root_id;
        for i in 0..50 {
            prev_id = create_diamond(i, vec![prev_id]);
        }

        rt.block_on(save_bonsai_changesets(bonsais, ctx.clone(), repo.clone()))
            .unwrap();

        derive_fastlog(ctx.clone(), repo.clone(), &mut rt, prev_id.clone());

        let terminator = |_cs_id| future::ready(Ok(false));
        let history = rt
            .block_on_std(list_file_history(
                ctx,
                repo,
                filepath,
                prev_id,
                Some(terminator),
            ))
            .unwrap();
        let history = rt.block_on_std(history.try_collect::<Vec<_>>()).unwrap();

        expected.reverse();
        assert_eq!(history, expected);
    }

    #[fbinit::test]
    fn test_list_history_terminator(fb: FacebookInit) {
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
        let mut rt = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock(fb);

        let filename = "1";
        let filepath = path(filename);

        let mut bonsais = vec![];
        let mut graph = HashMap::new();
        let mut create_branch = |branch, number, mut parents: Vec<_>, save_branch, branch_file| {
            let mut br = vec![];
            for i in 0..number {
                let content = format!("{} - {}", branch, i);
                let c = format!("{}", i);
                let mut changes = btreemap! { filename => Some(content.as_str()) };
                if branch_file {
                    changes.insert(branch, Some(c.as_str()));
                }

                let stored_files = rt.block_on_std(store_files(ctx.clone(), changes, repo.clone()));

                let bcs = create_bonsai_changeset_with_files(parents.clone(), stored_files);
                let bcs_id = bcs.get_changeset_id();
                bonsais.push(bcs);

                if save_branch {
                    br.push(bcs_id.clone());
                }
                graph.insert(bcs_id.clone(), parents);
                parents = vec![bcs_id];
            }
            (parents.get(0).unwrap().clone(), br)
        };

        let (a_top, mut a_branch) = create_branch("A", 20, vec![], true, false);
        let (b_top, _) = create_branch("B", 20, vec![], false, true);
        let (top, _) = create_branch("top", 100, vec![a_top, b_top], false, false);

        rt.block_on(save_bonsai_changesets(bonsais, ctx.clone(), repo.clone()))
            .unwrap();

        derive_fastlog(ctx.clone(), repo.clone(), &mut rt, top.clone());

        // prune all fastlog batch fetchings
        let terminator = Some(|_cs_id| future::ready(Ok(true)));
        let history = rt
            .block_on_std(list_file_history(
                ctx.clone(),
                repo.clone(),
                filepath.clone(),
                top.clone(),
                terminator,
            ))
            .unwrap();
        let history = rt.block_on_std(history.try_collect::<Vec<_>>()).unwrap();
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
        let history = rt
            .block_on_std(list_file_history(ctx, repo, filepath, top, terminator))
            .unwrap();
        let history = rt.block_on_std(history.try_collect::<Vec<_>>()).unwrap();

        // the beginning of the history should be same as bfs
        let expected = bfs(&graph, top);
        assert_eq!(history[..109], expected[..109]);

        // last 15 commits of the history should be last 15 of the branch A
        a_branch.reverse();
        assert_eq!(history[109..], a_branch[5..]);
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

    fn bfs(graph: &HashMap<ChangesetId, Vec<ChangesetId>>, node: ChangesetId) -> Vec<ChangesetId> {
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

    fn derive_fastlog(ctx: CoreContext, repo: BlobRepo, rt: &mut Runtime, bcs_id: ChangesetId) {
        rt.block_on(RootFastlog::derive(ctx, repo, bcs_id)).unwrap();
    }

    fn path(path_str: &str) -> Option<MPath> {
        MPath::new(path_str).ok()
    }
}
