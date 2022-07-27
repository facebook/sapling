/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::Repo;
use anyhow::anyhow;
use anyhow::format_err;
use anyhow::Error;
use async_trait::async_trait;
use blobstore::Loadable;
use blobstore::LoadableError;
use changeset_fetcher::ArcChangesetFetcher;
use changeset_fetcher::ChangesetFetcher;
use cloned::cloned;
use context::CoreContext;
use deleted_manifest::DeletedManifestOps;
use deleted_manifest::PathState;
use deleted_manifest::RootDeletedManifestV2Id;
use derived_data::BonsaiDerived;
use derived_data::DeriveError;
use fastlog::fetch_fastlog_batch_by_unode_id;
use fastlog::fetch_flattened;
use fastlog::FastlogParent;
use fastlog::RootFastlog;
use futures::future::try_join;
use futures::future::try_join_all;
use futures::stream;
use futures::stream::Stream as NewStream;
use futures_stats::futures03::TimedFutureExt;
use futures_util::StreamExt;
use futures_util::TryStreamExt;
use itertools::Itertools;
use manifest::Entry;
use manifest::ManifestOps;
use mononoke_types::ChangesetId;
use mononoke_types::FileUnodeId;
use mononoke_types::Generation;
use mononoke_types::MPath;
use mononoke_types::ManifestUnodeId;
use mutable_renames::MutableRenames;
use reachabilityindex::LeastCommonAncestorsHint;
use stats::prelude::*;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::sync::Arc;
use thiserror::Error;
use time_ext::DurationExt;
use unodes::RootUnodeManifestId;

define_stats! {
    prefix = "mononoke.fastlog";
    unexpected_existing_unode: timeseries(Sum),
    find_where_file_was_deleted_ms: timeseries(Sum, Average),
    merge_in_file_history: timeseries(Sum),
}

#[derive(Debug, Error)]
pub enum FastlogError {
    #[error("Internal error: {0}")]
    InternalError(String),
    #[error(transparent)]
    DeriveError(#[from] DeriveError),
    #[error(transparent)]
    LoadableError(#[from] LoadableError),
    #[error(transparent)]
    Error(#[from] Error),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum HistoryAcrossDeletions {
    Track,
    DontTrack,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FollowMutableFileHistory {
    MutableFileParents,
    ImmutableCommitParents,
}

pub type CsAndPath = (ChangesetId, Arc<Option<MPath>>);

pub enum NextChangeset {
    // Changeset is new and hasn't been returned
    // yet
    New(CsAndPath),
    // Changeset has already been returned,
    // so now we only need to process its parents
    AlreadyReturned(CsAndPath),
}

#[derive(Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct ParentOrder(usize);

pub enum TraversalOrder {
    SimpleGenNumOrder {
        next: Option<NextChangeset>,
        ctx: CoreContext,
        changeset_fetcher: ArcChangesetFetcher,
    },
    GenNumOrder {
        front_queue: VecDeque<NextChangeset>,
        // TODO(stash): ParentOrder is very basic at the moment,
        // and won't work correctly in all cases.
        heap: BinaryHeap<(Generation, Reverse<ParentOrder>, CsAndPath)>,
        ctx: CoreContext,
        changeset_fetcher: ArcChangesetFetcher,
    },
}

impl TraversalOrder {
    pub fn new_gen_num_order(ctx: CoreContext, changeset_fetcher: ArcChangesetFetcher) -> Self {
        Self::SimpleGenNumOrder {
            next: None,
            ctx,
            changeset_fetcher,
        }
    }

    async fn push_front(&mut self, cs_id: CsAndPath) -> Result<(), Error> {
        use TraversalOrder::*;

        match self {
            SimpleGenNumOrder { next, .. } => {
                debug_assert!(next.is_none());
                *next = Some(NextChangeset::AlreadyReturned(cs_id));
            }
            GenNumOrder { front_queue, .. } => {
                front_queue.push_front(NextChangeset::AlreadyReturned(cs_id));
            }
        }

        Ok(())
    }

    async fn push_ancestors(&mut self, cs_and_paths: &[CsAndPath]) -> Result<(), Error> {
        use TraversalOrder::*;

        if cs_and_paths.len() > 1 {
            STATS::merge_in_file_history.add_value(1);
        }

        let new_state: Option<TraversalOrder> = match self {
            SimpleGenNumOrder {
                next,
                ctx,
                changeset_fetcher,
            } => {
                if cs_and_paths.len() <= 1 {
                    if cs_and_paths.len() == 1 {
                        debug_assert!(next.is_none());
                        *next = Some(NextChangeset::New(cs_and_paths[0].clone()));
                    }
                    None
                } else {
                    // convert it to full-blown gen num ordering
                    let mut heap = BinaryHeap::new();
                    let cs_and_paths =
                        Self::convert_cs_ids(ctx, changeset_fetcher, cs_and_paths).await?;
                    heap.extend(cs_and_paths);
                    Some(TraversalOrder::GenNumOrder {
                        heap,
                        ctx: ctx.clone(),
                        changeset_fetcher: changeset_fetcher.clone(),
                        front_queue: VecDeque::new(),
                    })
                }
            }
            GenNumOrder {
                heap,
                ctx,
                changeset_fetcher,
                ..
            } => {
                let cs_and_paths =
                    Self::convert_cs_ids(ctx, changeset_fetcher, cs_and_paths).await?;
                heap.extend(cs_and_paths);
                None
            }
        };

        if let Some(new_state) = new_state {
            *self = new_state;
        }

        Ok(())
    }

    fn pop_front(&mut self) -> Option<NextChangeset> {
        use TraversalOrder::*;

        match self {
            SimpleGenNumOrder { next, .. } => next.take(),
            GenNumOrder {
                front_queue, heap, ..
            } => {
                let front = front_queue.pop_front();
                if front.is_some() {
                    return front;
                }
                heap.pop().map(|(_, _, cs_id)| NextChangeset::New(cs_id))
            }
        }
    }

    fn is_empty(&self) -> bool {
        use TraversalOrder::*;

        match self {
            SimpleGenNumOrder { next, .. } => next.is_none(),
            GenNumOrder {
                front_queue, heap, ..
            } => front_queue.is_empty() && heap.is_empty(),
        }
    }

    async fn convert_cs_ids(
        ctx: &CoreContext,
        changeset_fetcher: &ArcChangesetFetcher,
        cs_ids: &[CsAndPath],
    ) -> Result<Vec<(Generation, Reverse<ParentOrder>, CsAndPath)>, Error> {
        let cs_ids = try_join_all(cs_ids.iter().enumerate().map(
            |(num, (cs_id, path))| async move {
                let gen_num = changeset_fetcher
                    .get_generation_number(ctx.clone(), *cs_id)
                    .await?;
                Result::<_, Error>::Ok((gen_num, Reverse(ParentOrder(num)), (*cs_id, path.clone())))
            },
        ))
        .await?;

        Ok(cs_ids)
    }
}

async fn resolve_path_state(
    ctx: &CoreContext,
    repo: &impl Repo,
    cs_id: ChangesetId,
    path: &Option<MPath>,
) -> Result<Option<PathState>, Error> {
    RootDeletedManifestV2Id::resolve_path_state(ctx, repo.as_blob_repo(), cs_id, path).await
}

/// Returns a full history of the given path starting from the given unode in generation number order.
/// ```text
/// Accepts a `Visitor` object which controls the flow by filtering out the unwanted changesets
/// before they're added to the queue, see its docs for details. If you don't need to filter the
/// history you can provide `()` instead for default implementation.
///
/// This is the public API of this crate i.e. what clients should use if they want to
/// fetch the history.
///
/// If the path doesn't exist (or if the path never existed with history_across_deletions on) the
/// returned stream is empty.
///
/// Given a unode representing a commit-path `list_file_history` traverses commit history
/// In order to do this it keeps:
///   - history_graph: commit graph that is constructed from fastlog data and represents
///                    'child(cs_id) -> parents(cs_id)' relationship
///   - prefetch: changeset id to fetch fastlog batch for
///   - order: queue which stores commit in a valid order
///   - visited: set that marks nodes already enqueued
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
///   2 - perform traversal until the node for which parents haven't been prefetched
///   3 - stream all the "ready" nodes and set the last node to prefetch
/// The stream stops when there is nothing to return.
///
/// Why to pop all nodes on the same depth and not just one commit at a time?
/// Because if history contains merges and parents for more than one node on the current depth
/// haven't been fetched yet, we can fetch them at the same time using FuturesUnordered.
/// ```
pub async fn list_file_history(
    ctx: CoreContext,
    repo: &impl Repo,
    path: Option<MPath>,
    changeset_id: ChangesetId,
    mut visitor: impl Visitor,
    history_across_deletions: HistoryAcrossDeletions,
    mut follow_mutable_renames: FollowMutableFileHistory,
    mutable_renames: Arc<MutableRenames>,
    mut order: TraversalOrder,
) -> Result<impl NewStream<Item = Result<ChangesetId, Error>> + '_, FastlogError> {
    if tunables::tunables()
        .get_by_repo_fastlog_disable_mutable_renames(repo.repo_identity().name())
        .unwrap_or(false)
    {
        follow_mutable_renames = FollowMutableFileHistory::ImmutableCommitParents;
    }
    let path = Arc::new(path);

    let mut visited = HashSet::new();
    let mut history_graph = HashMap::new();
    let mut possible_mutable_ancestors_cache = Default::default();

    // The first step to find the last changeset that affected the given path.
    // get unode entry
    let resolved = resolve_path_state(&ctx, repo, changeset_id, &path).await?;
    // there might be more than one unode entry: if the given path was
    // deleted in several different branches, we have to gather history
    // from all of them
    let last_changesets = match resolved {
        Some(PathState::Deleted(deletion_nodes)) => {
            // we want to show commit, where path was deleted
            process_deletion_nodes(&ctx, repo, &mut history_graph, deletion_nodes, path.clone())
                .await?
        }
        Some(PathState::Exists(unode_entry)) => {
            fetch_linknodes_and_update_graph(
                &ctx,
                repo,
                vec![unode_entry],
                &mut history_graph,
                path.clone(),
            )
            .await?
        }
        None => return Ok(stream::iter(vec![]).boxed()),
    };
    // Find out if there's mutable rename between the current changeset and last
    // changeset that touched the path. (There's no such if the current
    // changeset and last changeset are the same commit.)
    let last_changesets = if follow_mutable_renames == FollowMutableFileHistory::MutableFileParents
        && !last_changesets
            .iter()
            .any(|x| x == &(changeset_id, path.clone()))
    {
        let possible_mutable_ancestors_for_path = find_possible_mutable_ancestors(
            &ctx,
            repo,
            path.as_ref(),
            &mut possible_mutable_ancestors_cache,
        )
        .await?;
        let (replacements, insertions) = replace_ancestors_with_mutable_ancestors(
            &ctx,
            repo,
            &(changeset_id, path),
            last_changesets,
            possible_mutable_ancestors_for_path,
        )
        .await?;
        history_graph.extend(insertions);
        replacements
    } else {
        last_changesets
    };

    visit(
        &ctx,
        repo,
        &mut visitor,
        None,
        last_changesets.clone(),
        &mut order,
        &mut visited,
    )
    .await?;

    // generate file history
    Ok(stream::try_unfold(
        // starting point
        TraversalState {
            history_graph,
            visited,
            order,
            prefetch: None,
            visitor,
            possible_mutable_ancestors_cache,
        },
        // unfold
        move |state| {
            cloned!(ctx, mutable_renames, repo);
            async move {
                do_history_unfold(
                    ctx.clone(),
                    repo.clone(),
                    state,
                    history_across_deletions,
                    follow_mutable_renames,
                    &mutable_renames,
                )
                .await
            }
        },
    )
    .map_ok(|history| stream::iter(history).map(Ok))
    .try_flatten()
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
    ///  * None in the first iteration
    ///  * common descendant of the ancestors that lead us to processing them.
    async fn visit(
        &mut self,
        ctx: &CoreContext,
        repo: &impl Repo,
        descendant_cs_id: Option<CsAndPath>,
        cs_and_paths: Vec<CsAndPath>,
    ) -> Result<Vec<CsAndPath>, Error>;

    /// May be called before visiting node so the visitor can prefetch neccesary
    /// data to make the visit faster.
    ///
    /// This funtion is not guaranteed to be called before each visit() call.
    //  The visit() is not guaranteed to be called later -  the traversal may terminat earlier.
    async fn preprocess(
        &mut self,
        _ctx: &CoreContext,
        _repo: &impl Repo,
        _descendant_id_cs_ids: Vec<(Option<CsAndPath>, Vec<CsAndPath>)>,
    ) -> Result<(), Error> {
        Ok(())
    }
}

#[async_trait]
impl Visitor for () {
    async fn visit(
        &mut self,
        _ctx: &CoreContext,
        _repo: &impl Repo,
        _descentant_cs_id: Option<CsAndPath>,
        cs_and_paths: Vec<CsAndPath>,
    ) -> Result<Vec<CsAndPath>, Error> {
        Ok(cs_and_paths)
    }
}

// Encapsulates all the things that should happen when the ancestors of a single history
// node are processed.
async fn visit(
    ctx: &CoreContext,
    repo: &impl Repo,
    visitor: &mut impl Visitor,
    cs_id: Option<CsAndPath>,
    ancestors: Vec<CsAndPath>,
    order: &mut TraversalOrder,
    visited: &mut HashSet<CsAndPath>,
) -> Result<(), FastlogError> {
    let ancestors = visitor.visit(ctx, repo, cs_id, ancestors).await?;
    let ancestors = ancestors
        .into_iter()
        .filter(|ancestor| visited.insert(ancestor.clone()))
        .collect::<Vec<_>>();
    order.push_ancestors(&ancestors).await?;
    Ok(())
}

type UnodeEntry = Entry<ManifestUnodeId, FileUnodeId>;

// Resolves the deletion nodes and inserts them into history as-if they were normal
// nodes being part of fastlog batch.
async fn process_deletion_nodes(
    ctx: &CoreContext,
    repo: &impl Repo,
    history_graph: &mut CommitGraph,
    deletion_nodes: Vec<(ChangesetId, UnodeEntry)>,
    path: Arc<Option<MPath>>,
) -> Result<Vec<CsAndPath>, FastlogError> {
    let mut deleted_linknodes = vec![];
    let mut last_unodes = vec![];

    for (deleted_linknode, last_unode_entry) in deletion_nodes {
        deleted_linknodes.push((deleted_linknode, path.clone()));
        last_unodes.push(last_unode_entry);
    }

    let last_linknodes =
        fetch_linknodes_and_update_graph(ctx, repo, last_unodes, history_graph, path.clone())
            .await?;
    let mut deleted_to_last_mapping: Vec<_> = deleted_linknodes
        .iter()
        .map(|(cs_id, _)| cs_id)
        .zip(last_linknodes.into_iter())
        .collect();
    deleted_to_last_mapping.sort_by_key(|(deleted_linknode, _)| *deleted_linknode);
    deleted_to_last_mapping
        .into_iter()
        .group_by(|(deleted_linknode, _)| **deleted_linknode)
        .into_iter()
        .for_each(|(deleted_linknode, grouped_last)| {
            history_graph.insert(
                (deleted_linknode, path.clone()),
                Some(grouped_last.map(|(_, last)| last).collect()),
            );
        });
    Ok(deleted_linknodes)
}

async fn fetch_linknodes_and_update_graph(
    ctx: &CoreContext,
    repo: &impl Repo,
    unode_entries: Vec<UnodeEntry>,
    history_graph: &mut CommitGraph,
    path: Arc<Option<MPath>>,
) -> Result<Vec<CsAndPath>, FastlogError> {
    let linknodes = unode_entries.into_iter().map({
        let path = &path;
        move |entry| async move {
            let unode = entry.load(ctx, repo.repo_blobstore()).await?;
            Ok::<_, FastlogError>(match unode {
                Entry::Tree(mf_unode) => (*mf_unode.linknode(), path.clone()),
                Entry::Leaf(file_unode) => (*file_unode.linknode(), path.clone()),
            })
        }
    });
    let linknodes = try_join_all(linknodes).await?;
    for linknode in &linknodes {
        history_graph.insert(linknode.clone(), None);
    }
    Ok(linknodes)
}

/// Returns history for a given unode if it exists.
async fn prefetch_history(
    ctx: &CoreContext,
    repo: &impl Repo,
    unode_entry: &UnodeEntry,
) -> Result<Option<Vec<(ChangesetId, Vec<FastlogParent>)>>, Error> {
    let maybe_fastlog_batch =
        fetch_fastlog_batch_by_unode_id(ctx, repo.repo_blobstore(), unode_entry).await?;
    if let Some(fastlog_batch) = maybe_fastlog_batch {
        let res = fetch_flattened(&fastlog_batch, ctx, repo.repo_blobstore()).await?;
        Ok(Some(res))
    } else {
        Ok(None)
    }
}

async fn derive_unode_entry(
    ctx: &CoreContext,
    repo: &impl Repo,
    cs_id: ChangesetId,
    path: &Option<MPath>,
) -> Result<Option<UnodeEntry>, Error> {
    let root_unode_mf_id = RootUnodeManifestId::derive(ctx, repo.as_blob_repo(), cs_id).await?;
    root_unode_mf_id
        .manifest_unode_id()
        .find_entry(ctx.clone(), repo.repo_blobstore_arc(), path.clone())
        .await
}

type CommitGraph = HashMap<CsAndPath, Option<Vec<CsAndPath>>>;
type PossibleMutableAncestorsCache = HashMap<Option<MPath>, Vec<(Generation, ChangesetId)>>;

struct TraversalState<V: Visitor> {
    history_graph: CommitGraph,
    visited: HashSet<CsAndPath>,
    order: TraversalOrder,
    prefetch: Option<CsAndPath>,
    visitor: V,
    possible_mutable_ancestors_cache: PossibleMutableAncestorsCache,
}

async fn do_history_unfold<V>(
    ctx: CoreContext,
    repo: &impl Repo,
    state: TraversalState<V>,
    history_across_deletions: HistoryAcrossDeletions,
    follow_mutable_renames: FollowMutableFileHistory,
    mutable_renames: &MutableRenames,
) -> Result<Option<(Vec<ChangesetId>, TraversalState<V>)>, Error>
where
    V: Visitor,
{
    let TraversalState {
        mut history_graph,
        mut visited,
        mut order,
        prefetch,
        mut visitor,
        mut possible_mutable_ancestors_cache,
    } = state;

    if let Some(ref prefetch) = prefetch {
        prefetch_and_process_history(
            &ctx,
            repo,
            &mut visitor,
            prefetch.clone(),
            &mut history_graph,
            follow_mutable_renames,
            &mut possible_mutable_ancestors_cache,
        )
        .await?;
    }

    let mut history = vec![];
    // process nodes to yield
    let mut next_to_fetch = None;
    while let Some(next_changeset) = order.pop_front() {
        let cs_and_path = match next_changeset {
            NextChangeset::New(cs_and_path) => {
                history.push(cs_and_path.0);
                cs_and_path
            }
            NextChangeset::AlreadyReturned(cs_and_path) => cs_and_path,
        };
        match history_graph.get(&cs_and_path) {
            Some(Some(parents)) => {
                // parents are fetched, ready to process
                let ancestors = if !parents.is_empty() {
                    parents.clone()
                } else {
                    try_continue_traversal_when_no_parents(
                        &ctx,
                        repo,
                        cs_and_path.clone(),
                        history_across_deletions,
                        &mut history_graph,
                        follow_mutable_renames,
                        mutable_renames,
                    )
                    .await?
                };

                visit(
                    &ctx,
                    repo,
                    &mut visitor,
                    Some(cs_and_path),
                    ancestors,
                    &mut order,
                    &mut visited,
                )
                .await?;
            }
            Some(None) | None => {
                // parents haven't been fetched yet
                // we want to proceed to next iteration to fetch the parents
                if Some(&cs_and_path) == prefetch.as_ref() {
                    return Err(format_err!(
                        "internal error: infinite loop while traversing history for {:?}",
                        cs_and_path
                    ));
                }
                next_to_fetch = Some(cs_and_path.clone());
                // Put it back in the queue so we can process once we fetch its history
                order.push_front(cs_and_path).await?;
                break;
            }
        }
    }

    // Terminate when there's nothing to return and nothing on BFS queue.
    if history.is_empty() && order.is_empty() {
        return Ok(None);
    }
    Ok(Some((
        history,
        TraversalState {
            history_graph,
            visited,
            order,
            prefetch: next_to_fetch,
            visitor,
            possible_mutable_ancestors_cache,
        },
    )))
}

async fn find_mutable_renames(
    ctx: &CoreContext,
    repo: &impl Repo,
    (cs_id, path): (ChangesetId, Arc<Option<MPath>>),
    history_graph: &mut CommitGraph,
    mutable_renames: &MutableRenames,
) -> Result<Vec<CsAndPath>, FastlogError> {
    if let Some(rename) = mutable_renames
        .get_rename(ctx, cs_id, (path.as_ref()).clone())
        .await?
    {
        let src_path = Arc::new(rename.src_path().cloned());
        // TODO(stash): this unode can be used to avoid unode manifest traversal
        // later while doing prefetching
        let src_unode = rename.src_unode().load(ctx, repo.repo_blobstore()).await?;
        let linknode = match src_unode {
            Entry::Tree(tree_unode) => *tree_unode.linknode(),
            Entry::Leaf(leaf_unode) => *leaf_unode.linknode(),
        };
        history_graph.insert((linknode, src_path.clone()), None);
        Ok(vec![(linknode, src_path)])
    } else {
        Ok(vec![])
    }
}

// Caches the list of possible mutable ancestors in memory - this shouldn't be a
// problem as long as the number of distinct paths in history is reasonable.
pub(crate) async fn find_possible_mutable_ancestors<'a>(
    ctx: &CoreContext,
    repo: &impl Repo,
    path: &Option<MPath>,
    possible_mutable_ancestors_cache: &'a mut PossibleMutableAncestorsCache,
) -> Result<&'a Vec<(Generation, ChangesetId)>, Error> {
    if !possible_mutable_ancestors_cache.contains_key(path) {
        let ancestors = _find_possible_mutable_ancestors(ctx, repo, path).await?;
        possible_mutable_ancestors_cache.insert(path.clone(), ancestors);
    }
    possible_mutable_ancestors_cache
        .get(path)
        .ok_or_else(|| anyhow!("programming error, mutable_ancestors_cache is missing a key"))
}

// Fetched the list of changeset with mutable renames for for path.
// Results are sorted by genration number.
pub(crate) async fn _find_possible_mutable_ancestors(
    ctx: &CoreContext,
    repo: &impl Repo,
    path: &Option<MPath>,
) -> Result<Vec<(Generation, ChangesetId)>, Error> {
    let mutable_renames = repo.mutable_renames();
    let mutable_csids = mutable_renames
        .get_cs_ids_with_rename(ctx, path.clone())
        .await?;
    let mut possible_mutable_ancestors: Vec<(Generation, ChangesetId)> =
        stream::iter(mutable_csids.into_iter())
            .then({
                move |mutated_at| async move {
                    // We also want to grab generation here, because we're going to sort
                    // by generation and consider "most recent" candidate first
                    let cs_gen = repo
                        .changeset_fetcher()
                        .get_generation_number(ctx.clone(), mutated_at)
                        .await?;
                    Ok::<_, Error>((cs_gen, mutated_at))
                }
            })
            .try_collect()
            .await?;
    // And turn the list of possible mutable ancestors into a stack sorted by generation
    possible_mutable_ancestors.sort_unstable_by_key(|(gen, _)| *gen);

    Ok(possible_mutable_ancestors)
}

/// Given list of new nodes to be inserted into the commit graph
/// augments and replaces them with mutable history data.
async fn augment_history_graph_insertions_with_mutable_ancestry(
    ctx: &CoreContext,
    repo: &impl Repo,
    mut graph_insertions: Vec<(CsAndPath, Option<Vec<CsAndPath>>)>,
    possible_mutable_ancestors_cache: &mut PossibleMutableAncestorsCache,
) -> Result<Vec<(CsAndPath, Option<Vec<CsAndPath>>)>, FastlogError> {
    // Start from splitting the insertions by path they are affecting.
    //
    // Usually we will only deal with a single path (unless the file was moved
    // in immutable history) but we have to handle the case of having multiple
    // paths.
    let mut new_insertions = vec![];
    graph_insertions.sort_unstable_by_key(|((_, path), _)| path.clone());
    let graph_insertions_by_path = graph_insertions
        .into_iter()
        .group_by(|((_, path), _)| path.clone());
    let graph_insertions_by_path = graph_insertions_by_path
        .into_iter()
        .map(|(k, v)| (k, v.collect::<Vec<_>>()))
        .collect::<Vec<_>>();

    for (path, graph_insertions) in graph_insertions_by_path.into_iter() {
        // Fetch all the points where the history was changed for the path.
        let possible_mutable_ancestors_for_path = find_possible_mutable_ancestors(
            ctx,
            repo,
            path.as_ref(),
            possible_mutable_ancestors_cache,
        )
        .await?;
        if !possible_mutable_ancestors_for_path.is_empty() {
            // If there are any, the for each node to-be-inserted into the graph
            // we try to find a mutable replacement.
            let replaced_insertions = try_join_all(graph_insertions.into_iter().map(
                |(cs_and_path, maybe_ancestors)| {
                    augment_single_history_graph_insertion_with_mutable_ancestry(
                        ctx,
                        repo,
                        cs_and_path,
                        maybe_ancestors,
                        possible_mutable_ancestors_for_path,
                    )
                },
            ))
            .await?;
            new_insertions.extend(replaced_insertions.into_iter().flatten());
        } else {
            // If there are none we do nothing.
            new_insertions.extend(graph_insertions);
        }
    }
    Ok(new_insertions)
}

/// Augments a single changeset->parents node with the information
/// from mutable renames.
///
/// Returns the resulting graph nodes to be inserted:
///  * when there is a mutable rename directly attached to the commit
//     present in immutable path history. We replace graph node with
//     one. For example:
//
//
//            C                      C
//            |                        \
//            B  D         =>        B  D
//            |                      |
//            A                      A
//
//
//    Mutable rename is redirecting the history from C to D) so we replace C->B
//    edge with C->D.
//
///  * when there is a mutable rename attached to the commit
//     not present in immutable path history (a commit where path was not
//     modified):
//
//
//            C                      C
//            ╷                      |
//           (X)                     X
//            ╷                        \
//            B  D         =>        B  D
//            |                      |
//            A                      A
//
//
//    Mutable rename is redirecting the history from X to D) but X is not present
//    in the path history so we replace C->B edge with C->X and X->D edges.
async fn augment_single_history_graph_insertion_with_mutable_ancestry(
    ctx: &CoreContext,
    repo: &impl Repo,
    cs_and_path: CsAndPath,
    immutable_ancestors: Option<Vec<CsAndPath>>,
    possible_mutable_ancestors_for_path: &Vec<(Generation, ChangesetId)>,
) -> Result<Vec<(CsAndPath, Option<Vec<CsAndPath>>)>, FastlogError> {
    if let Some(immutable_ancestors) = immutable_ancestors {
        let (res, mut insertions) = replace_ancestors_with_mutable_ancestors(
            ctx,
            repo,
            &cs_and_path,
            immutable_ancestors,
            possible_mutable_ancestors_for_path,
        )
        .await?;

        insertions.push((cs_and_path, Some(res)));
        Ok(insertions)
    } else {
        Ok(vec![(cs_and_path, None)])
    }
}

/// Given changeset path ancestors finds replacements in mutable ancestors
/// database.
///
/// Returns replacement and maybe extra history edges to be injected.
async fn replace_ancestors_with_mutable_ancestors(
    ctx: &CoreContext,
    repo: &impl Repo,
    cs_and_path: &CsAndPath,
    immutable_ancestors: Vec<CsAndPath>,
    possible_mutable_ancestors_for_path: &Vec<(Generation, ChangesetId)>,
) -> Result<(Vec<CsAndPath>, Vec<(CsAndPath, Option<Vec<CsAndPath>>)>), FastlogError> {
    let cs_id = cs_and_path.0;
    let path = &cs_and_path.1;
    if let Some((_, possible_ancestor_cs_id)) = possible_mutable_ancestors_for_path
        .iter()
        .find(|(_, possible_ancestor_cs_id)| cs_id == *possible_ancestor_cs_id)
    {
        // If the current commit is mutable rename destination. In that
        // case we need to redirect the history to the mutable source.
        let mutable_renames = repo.mutable_renames();
        if let Some(rename) = mutable_renames
            .get_rename(ctx, *possible_ancestor_cs_id, path.as_ref().clone())
            .await?
        {
            let src_unode = rename.src_unode().load(ctx, repo.repo_blobstore()).await?;
            let linknode = match src_unode {
                Entry::Tree(tree_unode) => *tree_unode.linknode(),
                Entry::Leaf(leaf_unode) => *leaf_unode.linknode(),
            };
            Ok((
                vec![(linknode, Arc::new(rename.src_path().cloned()))],
                vec![],
            ))
        } else {
            Err(anyhow!("inconsistency in mutable renames").into())
        }
    } else {
        let (replacements, graph_insertions): (
            _,
            Vec<Option<(CsAndPath, Option<Vec<CsAndPath>>)>>,
        ) = try_join_all(immutable_ancestors.iter().map(|immutable_ancestor| {
            replace_ancestor_with_mutable_ancestor(
                ctx,
                repo,
                cs_and_path,
                immutable_ancestor,
                possible_mutable_ancestors_for_path,
            )
        }))
        .await?
        .into_iter()
        .unzip();

        Ok((
            replacements,
            graph_insertions.into_iter().flatten().collect(),
        ))
    }
}

/// Given changeset patr ancestry relation find a replacement
/// in mutable ancestors database.
///
/// Returns replacement and maybe extra history edge to be injected.
async fn replace_ancestor_with_mutable_ancestor<'a>(
    ctx: &CoreContext,
    repo: &impl Repo,
    (cs_id, path): &CsAndPath,
    immutable_ancestor: &'a CsAndPath,
    possible_mutable_ancestors_for_path: &Vec<(Generation, ChangesetId)>,
) -> Result<(CsAndPath, Option<(CsAndPath, Option<Vec<CsAndPath>>)>), FastlogError> {
    let (immutable_ancestor_cs_id, immutable_ancestor_path) = immutable_ancestor;
    let skiplist_index = repo.skiplist_index();
    let mutable_renames = repo.mutable_renames();
    let changeset_fetcher = repo.changeset_fetcher();
    let (current_gen, immutable_ancestor_gen) = try_join(
        changeset_fetcher.get_generation_number(ctx.clone(), *cs_id),
        changeset_fetcher.get_generation_number(ctx.clone(), *immutable_ancestor_cs_id),
    )
    .await?;
    // For each possible mutable rename destination we have to check:
    for (possible_ancestor_gen, possible_ancestor_cs_id) in possible_mutable_ancestors_for_path {
        if *possible_ancestor_gen < current_gen && *possible_ancestor_gen > immutable_ancestor_gen {
            // If it's on the path between current commit and next immutable
            // ancestor.  We start from cheap generation number test to exclude
            // the most cases.  Then we do a real ancestry check.
            let res = try_join(
                skiplist_index.is_ancestor(
                    ctx,
                    &repo.changeset_fetcher_arc(),
                    *possible_ancestor_cs_id,
                    *cs_id,
                ),
                skiplist_index.is_ancestor(
                    ctx,
                    &repo.changeset_fetcher_arc(),
                    *immutable_ancestor_cs_id,
                    *possible_ancestor_cs_id,
                ),
            )
            .await?;
            if res.0 && res.1 {
                if let Some(rename) = mutable_renames
                    .get_rename(ctx, *possible_ancestor_cs_id, path.as_ref().clone())
                    .await?
                {
                    let src_unode = rename.src_unode().load(ctx, repo.repo_blobstore()).await?;
                    // The next node in path history doesn't have to be the src
                    // changeset. It needs to be the last commit that modified
                    // the path as of src changeset.
                    let linknode = match src_unode {
                        Entry::Tree(tree_unode) => *tree_unode.linknode(),
                        Entry::Leaf(leaf_unode) => *leaf_unode.linknode(),
                    };
                    return Ok((
                        // The extra node in ancestry path where the mutable rename is attached
                        (*possible_ancestor_cs_id, path.clone()),
                        // We also inject a link between that node and rename src we won't reach
                        // this node during the unode graph traversal.
                        Some((
                            (*possible_ancestor_cs_id, path.clone()),
                            Some(vec![(linknode, Arc::new(rename.src_path().cloned()))]),
                        )),
                    ));
                }
            }
        }
    }
    Ok((
        (*immutable_ancestor_cs_id, immutable_ancestor_path.clone()),
        None,
    ))
}

async fn try_continue_traversal_when_no_parents(
    ctx: &CoreContext,
    repo: &impl Repo,
    (cs_id, path): (ChangesetId, Arc<Option<MPath>>),
    history_across_deletions: HistoryAcrossDeletions,
    history_graph: &mut CommitGraph,
    follow_mutable_renames: FollowMutableFileHistory,
    mutable_renames: &MutableRenames,
) -> Result<Vec<CsAndPath>, FastlogError> {
    if history_across_deletions == HistoryAcrossDeletions::Track {
        let (stats, deletion_nodes) = find_where_file_was_deleted(ctx, repo, cs_id, &path)
            .timed()
            .await;
        STATS::find_where_file_was_deleted_ms
            .add_value(stats.completion_time.as_millis_unchecked() as i64);
        let deletion_nodes = deletion_nodes?;
        let deleted_nodes =
            process_deletion_nodes(ctx, repo, history_graph, deletion_nodes, path.clone()).await?;
        if !deleted_nodes.is_empty() {
            return Ok(deleted_nodes);
        }
    }

    if !tunables::tunables()
        .get_by_repo_fastlog_disable_mutable_renames(repo.repo_identity().name())
        .unwrap_or(follow_mutable_renames == FollowMutableFileHistory::ImmutableCommitParents)
    {
        return find_mutable_renames(ctx, repo, (cs_id, path), history_graph, mutable_renames)
            .await;
    }

    Ok(vec![])
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
    repo: &impl Repo,
    commit_no_more_history: ChangesetId,
    path: &Option<MPath>,
) -> Result<Vec<(ChangesetId, UnodeEntry)>, Error> {
    let parents = repo
        .as_blob_repo()
        .get_changeset_parents_by_bonsai(ctx.clone(), commit_no_more_history)
        .await?;

    let resolved_path_states = try_join_all(
        parents
            .into_iter()
            .map(|p_id| resolve_path_state(ctx, repo, p_id, path)),
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

/// Prefetches and processes fastlog batch for the given changeset id.
/// Handles the replacement of the history from fastlog batch with mutable
/// history when applicable.
async fn prefetch_and_process_history(
    ctx: &CoreContext,
    repo: &impl Repo,
    visitor: &mut impl Visitor,
    (changeset_id, path): (ChangesetId, Arc<Option<MPath>>),
    history_graph: &mut CommitGraph,
    follow_mutable_renames: FollowMutableFileHistory,
    possible_mutable_ancestors_cache: &mut PossibleMutableAncestorsCache,
) -> Result<(), Error> {
    let fastlog_batch = prefetch_fastlog_by_changeset(ctx, repo, changeset_id, &path).await?;
    let affected_changesets: Vec<_> = fastlog_batch.iter().map(|(cs_id, _)| *cs_id).collect();
    let mut graph_insertions = process_unode_batch(fastlog_batch, history_graph, path.clone());

    if follow_mutable_renames == FollowMutableFileHistory::MutableFileParents {
        graph_insertions = augment_history_graph_insertions_with_mutable_ancestry(
            ctx,
            repo,
            graph_insertions,
            possible_mutable_ancestors_cache,
        )
        .await?;
    }
    history_graph.extend(graph_insertions);

    visitor
        .preprocess(
            ctx,
            repo,
            affected_changesets
                .into_iter()
                .filter_map(|cs_id| {
                    history_graph
                        .get(&(cs_id, path.clone()))
                        .cloned()
                        .flatten()
                        .map(|parents| (Some((cs_id, path.clone())), parents))
                })
                .collect(),
        )
        .await?;
    Ok(())
}

fn process_unode_batch(
    unode_batch: Vec<(ChangesetId, Vec<FastlogParent>)>,
    graph: &CommitGraph,
    path: Arc<Option<MPath>>,
) -> Vec<(CsAndPath, Option<Vec<CsAndPath>>)> {
    let mut graph_insertions = Vec::new();
    for (cs_id, parents) in unode_batch {
        let has_unknown_parent = parents.iter().any(|parent| match parent {
            FastlogParent::Unknown => true,
            _ => false,
        });
        let known_parents: Vec<CsAndPath> = parents
            .into_iter()
            .filter_map(|parent| match parent {
                FastlogParent::Known(cs_id) => Some((cs_id, path.clone())),
                _ => None,
            })
            .collect();

        if let Some(maybe_parents) = graph.get(&(cs_id, path.clone())) {
            // history graph has the changeset
            if maybe_parents.is_none() && !has_unknown_parent {
                // the node was visited but had unknown parents
                // let's update the graph
                graph_insertions.push(((cs_id, path.clone()), Some(known_parents.clone())));
            }
        } else {
            // we haven't seen this changeset before
            if has_unknown_parent {
                // at least one parent is unknown ->
                // need to fetch unode batch for this changeset
                //
                // let's add to the graph with None parents, this way we mark the
                // changeset as visited for other traversal branches
                graph_insertions.push(((cs_id, path.clone()), None));
            } else {
                graph_insertions.push(((cs_id, path.clone()), Some(known_parents.clone())));
            }
        }
    }
    graph_insertions
}

async fn prefetch_fastlog_by_changeset(
    ctx: &CoreContext,
    repo: &impl Repo,
    changeset_id: ChangesetId,
    path: &Option<MPath>,
) -> Result<Vec<(ChangesetId, Vec<FastlogParent>)>, Error> {
    let unode_entry_opt = derive_unode_entry(ctx, repo, changeset_id.clone(), path).await?;
    let entry = unode_entry_opt
        .ok_or_else(|| format_err!("Unode entry is not found {:?} {:?}", changeset_id, path))?;

    // optimistically try to fetch history for a unode
    let fastlog_batch_opt = prefetch_history(ctx, repo, &entry).await?;
    if let Some(batch) = fastlog_batch_opt {
        return Ok(batch);
    }

    // if there is no history, let's try to derive batched fastlog data
    // and fetch history again
    RootFastlog::derive(ctx, repo.as_blob_repo(), changeset_id.clone()).await?;
    let fastlog_batch_opt = prefetch_history(ctx, repo, &entry).await?;
    fastlog_batch_opt
        .ok_or_else(|| format_err!("Fastlog data is not found {:?} {:?}", changeset_id, path))
}

#[cfg(test)]
mod test {
    use super::*;
    use blobrepo::AsBlobRepo;
    use blobrepo::BlobRepo;
    use bonsai_hg_mapping::BonsaiHgMapping;
    use bookmarks::Bookmarks;
    use changeset_fetcher::ChangesetFetcherArc;
    use changesets::Changesets;
    use changesets::ChangesetsRef;
    use context::CoreContext;
    use fastlog::RootFastlog;
    use fbinit::FacebookInit;
    use filestore::FilestoreConfig;
    use futures::future::FutureExt;
    use futures::future::TryFutureExt;
    use maplit::hashmap;
    use mutable_renames::MutableRenameEntry;
    use mutable_renames::MutableRenamesArc;
    use repo_blobstore::RepoBlobstore;
    use repo_derived_data::RepoDerivedData;
    use repo_identity::RepoIdentity;
    use repo_identity::RepoIdentityRef;
    use skiplist::SkiplistIndex;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use tests_utils::CreateCommitContext;
    use tunables::with_tunables_async_arc;

    #[facet::container]
    #[derive(Clone)]
    struct TestRepoWithMutableRenames {
        #[delegate(
            FilestoreConfig,
            RepoBlobstore,
            RepoIdentity,
            RepoDerivedData,
            dyn Bookmarks,
            dyn BonsaiHgMapping,
            dyn ChangesetFetcher,
            dyn Changesets,
        )]
        pub blob_repo: BlobRepo,

        #[facet]
        pub mutable_renames: MutableRenames,

        #[facet]
        pub skiplist_index: SkiplistIndex,
    }

    impl AsBlobRepo for TestRepoWithMutableRenames {
        fn as_blob_repo(&self) -> &BlobRepo {
            &self.blob_repo
        }
    }

    #[fbinit::test]
    async fn test_list_linear_history(fb: FacebookInit) -> Result<(), Error> {
        // generate couple of hundreds linear file changes and list history
        let repo: TestRepoWithMutableRenames = test_repo_factory::build_empty(fb).unwrap();
        let mutable_renames = repo.mutable_renames_arc();
        let blob_repo = repo.as_blob_repo();
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

        RootFastlog::derive(&ctx, blob_repo, top).await?;

        expected.reverse();
        check_history(
            ctx,
            &repo,
            path(filename),
            top,
            (),
            HistoryAcrossDeletions::Track,
            mutable_renames,
            FollowMutableFileHistory::ImmutableCommitParents,
            expected,
        )
        .await?;

        Ok(())
    }

    #[fbinit::test]
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

        let repo: TestRepoWithMutableRenames = test_repo_factory::build_empty(fb).unwrap();
        let mutable_renames = repo.mutable_renames_arc();
        let blob_repo = repo.as_blob_repo();
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

        RootFastlog::derive(&ctx, blob_repo, top).await?;

        let expected = bfs(&graph, top);
        check_history(
            ctx,
            &repo,
            path(filename),
            top,
            (),
            HistoryAcrossDeletions::Track,
            mutable_renames,
            FollowMutableFileHistory::ImmutableCommitParents,
            expected,
        )
        .await?;

        Ok(())
    }

    #[fbinit::test]
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

        let repo: TestRepoWithMutableRenames = test_repo_factory::build_empty(fb).unwrap();
        let mutable_renames = repo.mutable_renames_arc();
        let blob_repo = repo.as_blob_repo();
        let ctx = CoreContext::test_mock(fb);

        let filename = "1";
        let mut expected = vec![];

        let root_id = CreateCommitContext::new_root(&ctx, &blob_repo)
            .add_file(filename, "root")
            .commit()
            .await?;
        expected.push(root_id.clone());

        let mut prev_id = root_id;
        for _ in 0..50 {
            prev_id = create_diamond(&ctx, &repo, vec![prev_id], &mut expected).await?;
        }

        RootFastlog::derive(&ctx, blob_repo, prev_id).await?;

        expected.reverse();
        check_history(
            ctx,
            &repo,
            path(filename),
            prev_id,
            (),
            HistoryAcrossDeletions::Track,
            mutable_renames,
            FollowMutableFileHistory::ImmutableCommitParents,
            expected,
        )
        .await?;

        Ok(())
    }

    #[fbinit::test]
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
        let repo: TestRepoWithMutableRenames = test_repo_factory::build_empty(fb).unwrap();
        let mutable_renames = repo.mutable_renames_arc();
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

        #[derive(Clone)]
        struct NothingVisitor;
        #[async_trait]
        impl Visitor for NothingVisitor {
            async fn visit(
                &mut self,
                _ctx: &CoreContext,
                _repo: &impl Repo,
                _descendant_cs_id: Option<CsAndPath>,
                _cs_and_paths: Vec<CsAndPath>,
            ) -> Result<Vec<CsAndPath>, Error> {
                Ok(vec![])
            }
        }
        // history now should be empty - the visitor prevented traversal
        check_history(
            ctx.clone(),
            &repo,
            filepath.clone(),
            top.clone(),
            NothingVisitor {},
            HistoryAcrossDeletions::Track,
            mutable_renames.clone(),
            FollowMutableFileHistory::ImmutableCommitParents,
            vec![],
        )
        .await?;

        // prune right branch
        struct SingleBranchOfHistoryVisitor;
        #[async_trait]
        impl Visitor for SingleBranchOfHistoryVisitor {
            async fn visit(
                &mut self,
                _ctx: &CoreContext,
                _repo: &impl Repo,
                _descendant_cs_id: Option<CsAndPath>,
                cs_and_paths: Vec<CsAndPath>,
            ) -> Result<Vec<CsAndPath>, Error> {
                Ok(cs_and_paths.into_iter().next().into_iter().collect())
            }
        }
        let history = list_file_history(
            ctx.clone(),
            &repo,
            filepath,
            top,
            SingleBranchOfHistoryVisitor {},
            HistoryAcrossDeletions::Track,
            FollowMutableFileHistory::ImmutableCommitParents,
            mutable_renames,
            TraversalOrder::new_gen_num_order(ctx.clone(), repo.changeset_fetcher_arc()),
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

    #[fbinit::test]
    async fn test_list_history_deleted(fb: FacebookInit) -> Result<(), Error> {
        let repo: TestRepoWithMutableRenames = test_repo_factory::build_empty(fb).unwrap();
        let mutable_renames = repo.mutable_renames_arc();
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

        let history = |cs_id, path, expected| {
            cloned!(ctx, mutable_renames, repo);
            async move {
                check_history(
                    ctx.clone(),
                    &repo,
                    path,
                    cs_id,
                    (),
                    HistoryAcrossDeletions::Track,
                    mutable_renames,
                    FollowMutableFileHistory::ImmutableCommitParents,
                    expected,
                )
                .await?;

                Result::<_, Error>::Ok(())
            }
        };

        expected.reverse();
        // check deleted file
        history(bcs_id.clone(), path(filename), expected.clone()).await?;
        // check deleted directory
        history(bcs_id.clone(), path("dir"), expected.clone()).await?;

        // recreate dir and check
        let bcs_id = CreateCommitContext::new(&ctx, &repo, vec![bcs_id])
            .add_file("dir/otherfile", "boo")
            .commit()
            .await?;

        let mut res = vec![bcs_id];
        res.extend(expected);
        history(bcs_id.clone(), path("dir"), res).await?;

        Ok(())
    }

    #[fbinit::test]
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
        let repo: TestRepoWithMutableRenames = test_repo_factory::build_empty(fb).unwrap();
        let mutable_renames = repo.mutable_renames_arc();
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

        let history = |cs_id, path, expected| {
            cloned!(ctx, mutable_renames, repo);
            async move {
                check_history(
                    ctx.clone(),
                    &repo,
                    path,
                    cs_id,
                    (),
                    HistoryAcrossDeletions::Track,
                    mutable_renames,
                    FollowMutableFileHistory::ImmutableCommitParents,
                    expected,
                )
                .await?;
                Result::<_, Error>::Ok(())
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
        history(l.clone(), path("file"), expected).await?;

        let expected = vec![i.clone(), g.clone(), c.clone(), d.clone()];
        history(l.clone(), path("dir/file"), expected).await?;

        let expected = vec![k.clone(), i.clone(), b.clone(), c.clone()];
        history(l.clone(), path("dir_1/file_1"), expected).await?;

        let expected = vec![
            k.clone(),
            i.clone(),
            g.clone(),
            c.clone(),
            d.clone(),
            b.clone(),
        ];
        history(l.clone(), path("dir_1"), expected).await?;

        Ok(())
    }

    #[fbinit::test]
    async fn test_list_history_across_deletions_linear(fb: FacebookInit) -> Result<(), Error> {
        let repo: TestRepoWithMutableRenames = test_repo_factory::build_empty(fb).unwrap();
        let mutable_renames = repo.mutable_renames_arc();
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

        let expected = expected.into_iter().rev().collect::<Vec<_>>();
        check_history(
            ctx.clone(),
            &repo,
            MPath::new_opt(filename)?,
            bcs_id,
            (),
            HistoryAcrossDeletions::Track,
            mutable_renames.clone(),
            FollowMutableFileHistory::ImmutableCommitParents,
            expected,
        )
        .await?;

        check_history(
            ctx.clone(),
            &repo,
            MPath::new_opt(filename)?,
            bcs_id,
            (),
            HistoryAcrossDeletions::DontTrack,
            mutable_renames,
            FollowMutableFileHistory::ImmutableCommitParents,
            vec![bcs_id],
        )
        .await?;

        Ok(())
    }

    #[fbinit::test]
    async fn test_list_history_across_deletions_with_merges(fb: FacebookInit) -> Result<(), Error> {
        let repo: TestRepoWithMutableRenames = test_repo_factory::build_empty(fb).unwrap();
        let mutable_renames = repo.mutable_renames_arc();
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

        let mut expected = expected.into_iter().rev().collect::<Vec<_>>();
        check_history(
            ctx.clone(),
            &repo,
            MPath::new_opt(filename)?,
            bcs_id,
            (),
            HistoryAcrossDeletions::Track,
            mutable_renames.clone(),
            FollowMutableFileHistory::ImmutableCommitParents,
            expected.clone(),
        )
        .await?;

        // Now check the history starting from a merge commit
        expected.remove(0);
        check_history(
            ctx.clone(),
            &repo,
            MPath::new_opt(filename)?,
            merge,
            (),
            HistoryAcrossDeletions::Track,
            mutable_renames,
            FollowMutableFileHistory::ImmutableCommitParents,
            expected,
        )
        .await?;

        Ok(())
    }

    #[fbinit::test]
    async fn test_list_history_with_mutable_renames(fb: FacebookInit) -> Result<(), Error> {
        let repo: TestRepoWithMutableRenames = test_repo_factory::build_empty(fb).unwrap();
        let mutable_renames = repo.mutable_renames_arc();
        let ctx = CoreContext::test_mock(fb);

        let first_src_filename = "dir/1";
        let first_dst_filename = "dir2/2";

        let second_src_filename = "file";
        let second_dst_filename = "moved_file";

        let first_bcs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file(first_src_filename, "content1")
            .add_file(second_src_filename, "content1")
            .commit()
            .await?;
        let second_bcs_id = CreateCommitContext::new(&ctx, &repo, vec![first_bcs_id])
            .add_file(first_src_filename, "content2")
            .commit()
            .await?;
        let third_bcs_id = CreateCommitContext::new(&ctx, &repo, vec![second_bcs_id])
            .delete_file(first_src_filename)
            .delete_file(second_src_filename)
            .add_file(first_dst_filename, "content3")
            .add_file(second_dst_filename, "content3")
            .commit()
            .await?;

        //    0 <- removes "dir/1", "file"; adds "dir2/2", "moved_file"
        //    |
        //    0  <- modifies "dir/1"
        //    |
        //    0  <- creates "dir/1", "file"

        // No mutable renames - just a single commit is returned
        check_history(
            ctx.clone(),
            &repo,
            MPath::new_opt(first_dst_filename)?,
            third_bcs_id,
            (),
            HistoryAcrossDeletions::Track,
            mutable_renames.clone(),
            FollowMutableFileHistory::ImmutableCommitParents,
            vec![third_bcs_id],
        )
        .await?;

        check_history(
            ctx.clone(),
            &repo,
            MPath::new_opt(second_dst_filename)?,
            third_bcs_id,
            (),
            HistoryAcrossDeletions::Track,
            mutable_renames.clone(),
            FollowMutableFileHistory::ImmutableCommitParents,
            vec![third_bcs_id],
        )
        .await?;

        // Set mutable renames
        let first_src_unode = derive_unode_entry(
            &ctx,
            &repo,
            second_bcs_id,
            &MPath::new_opt(first_src_filename)?,
        )
        .await?
        .ok_or_else(|| format_err!("not found source unode id"))?;

        let second_src_unode = derive_unode_entry(
            &ctx,
            &repo,
            second_bcs_id,
            &MPath::new_opt(second_src_filename)?,
        )
        .await?
        .ok_or_else(|| format_err!("not found source unode id"))?;

        mutable_renames
            .add_or_overwrite_renames(
                &ctx,
                repo.changesets(),
                vec![
                    MutableRenameEntry::new(
                        third_bcs_id,
                        MPath::new_opt(first_dst_filename)?,
                        second_bcs_id,
                        MPath::new_opt(first_src_filename)?,
                        first_src_unode,
                    )?,
                    MutableRenameEntry::new(
                        third_bcs_id,
                        MPath::new_opt(second_dst_filename)?,
                        second_bcs_id,
                        MPath::new_opt(second_src_filename)?,
                        second_src_unode,
                    )?,
                ],
            )
            .await?;

        let tunables = tunables::MononokeTunables::default();
        tunables.update_by_repo_bools(&hashmap! {
            repo.repo_identity().name().to_string() => hashmap! {
                "fastlog_disable_mutable_renames".to_string() => true,
            },
        });
        let tunables = Arc::new(tunables);
        // Tunable is not enabled, so result is the same
        let actual = check_history(
            ctx.clone(),
            &repo,
            MPath::new_opt(first_dst_filename)?,
            third_bcs_id,
            (),
            HistoryAcrossDeletions::Track,
            mutable_renames.clone(),
            FollowMutableFileHistory::MutableFileParents,
            vec![third_bcs_id],
        );
        with_tunables_async_arc(tunables.clone(), actual.boxed()).await?;

        let actual = check_history(
            ctx.clone(),
            &repo,
            MPath::new_opt(second_dst_filename)?,
            third_bcs_id,
            (),
            HistoryAcrossDeletions::Track,
            mutable_renames.clone(),
            FollowMutableFileHistory::MutableFileParents,
            vec![third_bcs_id],
        );
        with_tunables_async_arc(tunables.clone(), actual.boxed()).await?;

        // Now check the actual mutable history.
        check_history(
            ctx.clone(),
            &repo,
            MPath::new_opt(first_dst_filename)?,
            third_bcs_id,
            (),
            HistoryAcrossDeletions::Track,
            mutable_renames.clone(),
            FollowMutableFileHistory::MutableFileParents,
            vec![third_bcs_id, second_bcs_id, first_bcs_id],
        )
        .await?;

        check_history(
            ctx.clone(),
            &repo,
            MPath::new_opt(second_dst_filename)?,
            third_bcs_id,
            (),
            HistoryAcrossDeletions::Track,
            mutable_renames,
            FollowMutableFileHistory::MutableFileParents,
            vec![third_bcs_id, first_bcs_id],
        )
        .await?;

        Ok(())
    }

    #[fbinit::test]
    async fn test_list_history_with_mutable_renames_attached_to_unrelated_commits(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let repo: TestRepoWithMutableRenames = test_repo_factory::build_empty(fb).unwrap();
        let mutable_renames = repo.mutable_renames_arc();
        let ctx = CoreContext::test_mock(fb);

        let first_src_filename = "dir/1";
        let first_dst_filename = "dir2/2";

        let second_src_filename = "file";
        let second_dst_filename = "moved_file";

        let unrelated_filename = "unrelated";

        let first_bcs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file(first_src_filename, "content1")
            .add_file(second_src_filename, "content1")
            .commit()
            .await?;
        let second_bcs_id = CreateCommitContext::new(&ctx, &repo, vec![first_bcs_id])
            .add_file(first_src_filename, "content2")
            .add_file(first_dst_filename, "content2")
            .commit()
            .await?;

        let third_bcs_id = CreateCommitContext::new(&ctx, &repo, vec![second_bcs_id])
            .add_file(unrelated_filename, "unrelated content3")
            .commit()
            .await?;

        let fourth_bcs_id = CreateCommitContext::new(&ctx, &repo, vec![third_bcs_id])
            .delete_file(first_src_filename)
            .delete_file(second_src_filename)
            .add_file(first_dst_filename, "content4")
            .add_file(second_dst_filename, "content4")
            .commit()
            .await?;

        let fifth_bcs_id = CreateCommitContext::new(&ctx, &repo, vec![fourth_bcs_id])
            .add_file(unrelated_filename, "unrelated content5")
            .commit()
            .await?;

        let sixth_bcs_id = CreateCommitContext::new(&ctx, &repo, vec![fifth_bcs_id])
            .add_file(unrelated_filename, "unrelated content6")
            .commit()
            .await?;

        //    6 <- modifies "unrelated"
        //    |
        //    5 <- modifies "unrelated"
        //    |
        //    4 <- removes "dir/1", "file"; adds "moved_file"; modifies "dir2/2"
        //    |
        //    3 <- modifies "unrelated"
        //    |
        //    2  <- modifies "dir/1", adds "dir2/2"
        //    |
        //    1  <- creates "dir/1", "file"
        //
        // mutable renames:
        //  * dir/1 from 1st changeset -> dir2/2 from 3rd changeset
        //  * file from 2nd changeset -> moved_file from 5th changeset

        // Set mutable renames
        let first_src_unode = derive_unode_entry(
            &ctx,
            &repo,
            first_bcs_id,
            &MPath::new_opt(first_src_filename)?,
        )
        .await?
        .ok_or_else(|| format_err!("not found source unode id"))?;

        let second_src_unode = derive_unode_entry(
            &ctx,
            &repo,
            second_bcs_id,
            &MPath::new_opt(second_src_filename)?,
        )
        .await?
        .ok_or_else(|| format_err!("not found source unode id"))?;

        mutable_renames
            .add_or_overwrite_renames(
                &ctx,
                repo.changesets(),
                vec![
                    MutableRenameEntry::new(
                        third_bcs_id,
                        MPath::new_opt(first_dst_filename)?,
                        first_bcs_id,
                        MPath::new_opt(first_src_filename)?,
                        first_src_unode,
                    )?,
                    MutableRenameEntry::new(
                        fifth_bcs_id,
                        MPath::new_opt(second_dst_filename)?,
                        second_bcs_id,
                        MPath::new_opt(second_src_filename)?,
                        second_src_unode,
                    )?,
                ],
            )
            .await?;

        check_history(
            ctx.clone(),
            &repo,
            MPath::new_opt(first_dst_filename)?,
            sixth_bcs_id,
            (),
            HistoryAcrossDeletions::Track,
            mutable_renames.clone(),
            FollowMutableFileHistory::MutableFileParents,
            vec![fourth_bcs_id, third_bcs_id, first_bcs_id],
        )
        .await?;

        check_history(
            ctx.clone(),
            &repo,
            MPath::new_opt(second_dst_filename)?,
            sixth_bcs_id,
            (),
            HistoryAcrossDeletions::Track,
            mutable_renames,
            FollowMutableFileHistory::MutableFileParents,
            vec![fifth_bcs_id, first_bcs_id],
        )
        .await?;

        Ok(())
    }

    #[fbinit::test]
    async fn test_different_order(fb: FacebookInit) -> Result<(), Error> {
        let repo: TestRepoWithMutableRenames = test_repo_factory::build_empty(fb).unwrap();
        let mutable_renames = repo.mutable_renames_arc();
        let ctx = CoreContext::test_mock(fb);

        let filename = "dir/1";

        //   O
        //  / \
        //  O  |
        //  |  |
        //  O  O
        //  \ /
        //   O

        let bcs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file(filename, "content1")
            .commit()
            .await?;

        let first_left_bcs_id = CreateCommitContext::new(&ctx, &repo, vec![bcs_id])
            .add_file(filename, "leftcontent1")
            .commit()
            .await?;
        let second_left_bcs_id = CreateCommitContext::new(&ctx, &repo, vec![first_left_bcs_id])
            .add_file(filename, "leftcontent2")
            .commit()
            .await?;

        let right_bcs_id = CreateCommitContext::new(&ctx, &repo, vec![bcs_id])
            .add_file(filename, "rightcontent1")
            .commit()
            .await?;

        let merge = CreateCommitContext::new(&ctx, &repo, vec![second_left_bcs_id, right_bcs_id])
            .add_file(filename, "merge")
            .commit()
            .await?;

        let expected_gen_num = vec![
            merge,
            second_left_bcs_id,
            first_left_bcs_id,
            right_bcs_id,
            bcs_id,
        ];

        let history_stream = list_file_history(
            ctx.clone(),
            &repo,
            MPath::new_opt(filename)?,
            merge,
            (),
            HistoryAcrossDeletions::Track,
            FollowMutableFileHistory::ImmutableCommitParents,
            mutable_renames.clone(),
            TraversalOrder::new_gen_num_order(ctx.clone(), repo.changeset_fetcher_arc()),
        )
        .await?;

        let actual = history_stream.try_collect::<Vec<_>>().await?;
        assert_eq!(actual, expected_gen_num);

        Ok(())
    }

    #[fbinit::test]
    async fn test_simple_gen_num(fb: FacebookInit) -> Result<(), Error> {
        let repo: TestRepoWithMutableRenames = test_repo_factory::build_empty(fb).unwrap();
        let mutable_renames = repo.mutable_renames_arc();
        let ctx = CoreContext::test_mock(fb);

        let filename = "dir/1";

        //   O
        //   |
        //   O
        //   |
        //   O
        //   |
        //   O

        let mut expected = vec![];
        let bcs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file(filename, "content1")
            .commit()
            .await?;
        expected.push(bcs_id);

        let bcs_id = CreateCommitContext::new(&ctx, &repo, vec![bcs_id])
            .add_file(filename, "content2")
            .commit()
            .await?;
        expected.push(bcs_id);

        let bcs_id = CreateCommitContext::new(&ctx, &repo, vec![bcs_id])
            .add_file(filename, "content3")
            .commit()
            .await?;
        expected.push(bcs_id);

        let bcs_id = CreateCommitContext::new(&ctx, &repo, vec![bcs_id])
            .add_file(filename, "content4")
            .commit()
            .await?;
        expected.push(bcs_id);
        expected.reverse();

        let get_gen_number_count = Arc::new(AtomicUsize::new(0));
        let cs_fetcher = CountingChangesetFetcher::new(
            repo.changeset_fetcher_arc(),
            get_gen_number_count.clone(),
        );

        let history_stream = list_file_history(
            ctx.clone(),
            &repo,
            MPath::new_opt(filename)?,
            bcs_id,
            (),
            HistoryAcrossDeletions::Track,
            FollowMutableFileHistory::ImmutableCommitParents,
            mutable_renames.clone(),
            TraversalOrder::new_gen_num_order(ctx, Arc::new(cs_fetcher)),
        )
        .await?;

        let actual = history_stream.try_collect::<Vec<_>>().await?;
        assert_eq!(actual, expected);

        assert_eq!(get_gen_number_count.load(Ordering::Relaxed), 0);

        Ok(())
    }

    struct CountingChangesetFetcher {
        cs_fetcher: ArcChangesetFetcher,
        pub get_gen_number_count: Arc<AtomicUsize>,
    }

    impl CountingChangesetFetcher {
        fn new(cs_fetcher: ArcChangesetFetcher, get_gen_number_count: Arc<AtomicUsize>) -> Self {
            Self {
                cs_fetcher,
                get_gen_number_count,
            }
        }
    }

    #[async_trait]
    impl ChangesetFetcher for CountingChangesetFetcher {
        async fn get_generation_number(
            &self,
            ctx: CoreContext,
            cs_id: ChangesetId,
        ) -> Result<Generation, Error> {
            self.get_gen_number_count.fetch_add(1, Ordering::Relaxed);
            self.cs_fetcher.get_generation_number(ctx, cs_id).await
        }

        async fn get_parents(
            &self,
            ctx: CoreContext,
            cs_id: ChangesetId,
        ) -> Result<Vec<ChangesetId>, Error> {
            self.cs_fetcher.get_parents(ctx, cs_id).await
        }
    }

    type TestCommitGraph = HashMap<ChangesetId, Vec<ChangesetId>>;

    async fn create_branch(
        ctx: &CoreContext,
        repo: &TestRepoWithMutableRenames,
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
        repo: &TestRepoWithMutableRenames,
        parents: Vec<ChangesetId>,
        expected: &mut Vec<ChangesetId>,
    ) -> Result<ChangesetId, Error> {
        let filename = "1";
        // bottom
        let bottom_id = CreateCommitContext::new(&ctx, repo, parents.clone())
            .add_file(filename, format!("B - {:?}", parents))
            .commit()
            .await?;
        expected.push(bottom_id.clone());

        // right
        let right_id = CreateCommitContext::new(&ctx, repo, vec![bottom_id])
            .add_file(filename, format!("R - {:?}", parents))
            .commit()
            .await?;
        expected.push(right_id.clone());

        // left
        let left_id = CreateCommitContext::new(&ctx, repo, vec![bottom_id])
            .add_file(filename, format!("L - {:?}", parents))
            .commit()
            .await?;
        expected.push(left_id.clone());

        // up
        let up_id = CreateCommitContext::new(&ctx, repo, vec![left_id, right_id])
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

    async fn check_history(
        ctx: CoreContext,
        repo: &TestRepoWithMutableRenames,
        path: Option<MPath>,
        changeset_id: ChangesetId,
        visitor: impl Visitor + Clone,
        history_across_deletions: HistoryAcrossDeletions,
        mutable_renames: Arc<MutableRenames>,
        follow_mutable_file_history: FollowMutableFileHistory,
        expected: Vec<ChangesetId>,
    ) -> Result<(), Error> {
        let history = list_file_history(
            ctx.clone(),
            repo,
            path,
            changeset_id,
            visitor,
            history_across_deletions,
            follow_mutable_file_history,
            mutable_renames,
            TraversalOrder::new_gen_num_order(ctx.clone(), repo.changeset_fetcher_arc()),
        )
        .await?
        .try_collect::<Vec<_>>()
        .await?;

        let mut prev_gen_num = None;
        for cs_id in &history {
            let new_gen_num = repo
                .changeset_fetcher_arc()
                .get_generation_number(ctx.clone(), *cs_id)
                .await?;
            if let Some(prev_gen_num) = prev_gen_num {
                assert!(prev_gen_num >= new_gen_num);
            }
            prev_gen_num = Some(new_gen_num);
        }

        assert_eq!(
            history.into_iter().collect::<HashSet<_>>(),
            expected.into_iter().collect::<HashSet<_>>(),
        );

        Ok(())
    }
}
