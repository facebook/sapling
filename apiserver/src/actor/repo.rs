/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::{
    cmp,
    collections::{BTreeMap, HashMap, HashSet},
    convert::{TryFrom, TryInto},
    sync::{Arc, Mutex},
};

use blobrepo::{file_history::get_file_history, BlobRepo};
use blobrepo_factory::{open_blobrepo, Caching};
use blobstore::Loadable;
use bookmarks::BookmarkName;
use cloned::cloned;
use context::CoreContext;
use derived_data::BonsaiDerived;
use failure::Error;
use fastlog::{prefetch_history, FastlogParent, RootFastlog, RootFastlogMapping};
use fbinit::FacebookInit;
use futures::{
    future::{self, err, join_all, ok},
    lazy,
    stream::{futures_ordered, iter_ok, FuturesUnordered},
    Future, IntoFuture, Stream,
};
use futures_ext::{
    bounded_traversal::bounded_traversal_dag, try_boxfuture, BoxFuture, FutureExt, StreamExt,
};
use futures_stats::{FutureStats, Timed};
use manifest::{Entry as ManifestEntry, ManifestOps};
use remotefilelog::create_getpack_v1_blob;
use repo_client::gettreepack_entries;
use slog::{debug, Logger};
use time_ext::DurationExt;
use unodes::{RootUnodeManifestId, RootUnodeManifestMapping};

use mercurial_types::{
    blobs::HgBlobChangeset, manifest::Content, HgChangesetId, HgEntry, HgFileNodeId, HgManifestId,
};
use metaconfig_types::{CommonConfig, RepoConfig};
use scuba_ext::{ScubaSampleBuilder, ScubaSampleBuilderExt};
use stats::{define_stats, Timeseries};
use types::{
    api::{DataRequest, DataResponse, HistoryRequest, HistoryResponse, TreeRequest},
    DataEntry, Key, RepoPathBuf, WireHistoryEntry,
};
use warm_bookmarks_cache::WarmBookmarksCache;

use mononoke_types::{ChangesetId, FileUnodeId, MPath, ManifestUnodeId};
use reachabilityindex::ReachabilityIndex;
use skiplist::{fetch_skiplist_index, SkiplistIndex};

// Purely so that we can build new-style API objects from old style
use futures_preview::future::{FutureExt as _, TryFutureExt};
use mononoke_api::repo::open_synced_commit_mapping;
use synced_commit_mapping::SyncedCommitMapping;

use crate::cache::CacheManager;
use crate::errors::ErrorKind;
use crate::from_string as FS;

use super::file_stream::IntoFileStream;
use super::model::{Entry, EntryLight, EntryWithSizeAndContentHash};
use super::{MononokeRepoQuery, MononokeRepoResponse, Revision};

define_stats! {
    prefix = "mononoke.apiserver.repo";
    get_raw_file: timeseries(RATE, SUM),
    get_blob_content: timeseries(RATE, SUM),
    list_directory: timeseries(RATE, SUM),
    list_directory_unodes: timeseries(RATE, SUM),
    get_tree: timeseries(RATE, SUM),
    get_changeset: timeseries(RATE, SUM),
    get_branches: timeseries(RATE, SUM),
    get_file_history: timeseries(RATE, SUM),
    get_last_commit_on_path: timeseries(RATE, SUM),
    is_ancestor: timeseries(RATE, SUM),
    eden_get_data: timeseries(RATE, SUM),
    eden_get_history: timeseries(RATE, SUM),
    eden_get_trees: timeseries(RATE, SUM),
    eden_prefetch_trees: timeseries(RATE, SUM),
}

#[derive(Clone)]
pub struct MononokeRepo {
    pub(crate) repo: BlobRepo,
    logger: Logger,
    pub(crate) skiplist_index: Arc<SkiplistIndex>,
    cache: Option<CacheManager>,
    pub(crate) unodes_derived_mapping: Arc<RootUnodeManifestMapping>,
    // Cached public bookmarks that are used by apiserver. They can be outdated but not by much
    // (normally just a few seconds).
    // These bookmarks are updated when derived data is generated for them.
    warm_bookmarks_cache: WarmBookmarksCache,
    // Needed for the current way to create a new Mononoke object
    pub(crate) synced_commit_mapping: Arc<dyn SyncedCommitMapping>,
}

impl MononokeRepo {
    pub fn new(
        fb: FacebookInit,
        logger: Logger,
        config: RepoConfig,
        common_config: CommonConfig,
        myrouter_port: Option<u16>,
        cache: Option<CacheManager>,
        with_cachelib: Caching,
        with_skiplist: bool,
    ) -> impl Future<Item = Self, Error = Error> {
        let ctx = CoreContext::new_with_logger(fb, logger.clone());

        let skiplist_index_blobstore_key = config.skiplist_index_blobstore_key.clone();

        let repoid = config.repoid;

        // This is hacky, for the benefit of the new Mononoke object type
        open_synced_commit_mapping(config.clone(), myrouter_port)
            .boxed()
            .compat()
            .join(open_blobrepo(
                fb,
                config.storage_config.clone(),
                repoid,
                myrouter_port,
                with_cachelib,
                config.bookmarks_cache_ttl,
                config.redaction,
                common_config.scuba_censored_table,
                config.filestore,
                logger.clone(),
            ))
            .map(move |(synced_commit_mapping, repo)| {
                let warm_bookmarks_cache =
                    WarmBookmarksCache::new(ctx.clone(), logger.clone(), repo.clone());

                let skiplist_index = {
                    if !with_skiplist {
                        ok(Arc::new(SkiplistIndex::new())).right_future()
                    } else {
                        fetch_skiplist_index(
                            ctx.clone(),
                            skiplist_index_blobstore_key,
                            repo.get_blobstore().boxed(),
                        )
                        .left_future()
                    }
                };

                skiplist_index.join(warm_bookmarks_cache).map(
                    move |(skiplist_index, warm_bookmarks_cache)| {
                        let unodes_derived_mapping =
                            Arc::new(RootUnodeManifestMapping::new(repo.get_blobstore()));
                        Self {
                            repo,
                            logger,
                            skiplist_index,
                            cache,
                            unodes_derived_mapping,
                            warm_bookmarks_cache,
                            synced_commit_mapping: Arc::new(synced_commit_mapping),
                        }
                    },
                )
            })
            .flatten()
    }

    fn get_hgchangesetid_from_revision(
        &self,
        ctx: CoreContext,
        revision: Revision,
    ) -> BoxFuture<HgChangesetId, Error> {
        let repo = self.repo.clone();
        match revision {
            Revision::CommitHash(hash) => {
                FS::get_changeset_id(hash).into_future().from_err().boxify()
            }
            Revision::Bookmark(bookmark) => self
                .get_bonsai_id_from_bookmark(ctx.clone(), bookmark)
                .from_err()
                .and_then(move |bcs_id| repo.get_hg_from_bonsai_changeset(ctx, bcs_id))
                .boxify(),
        }
    }

    fn get_bonsai_id_from_revision(
        &self,
        ctx: CoreContext,
        revision: Revision,
    ) -> impl Future<Item = ChangesetId, Error = ErrorKind> {
        let repo = self.repo.clone();

        match revision {
            Revision::CommitHash(hash) => FS::get_changeset_id(hash)
                .into_future()
                .from_err()
                .and_then({
                    cloned!(ctx, repo);
                    move |changesetid| {
                        repo.get_bonsai_from_hg(ctx, changesetid)
                            .from_err()
                            .and_then(move |maybe_bcs_id| {
                                maybe_bcs_id
                                    .ok_or(ErrorKind::NotFound(format!("{}", changesetid), None))
                            })
                    }
                })
                .boxify(),
            Revision::Bookmark(bookmark) => self.get_bonsai_id_from_bookmark(ctx, bookmark),
        }
    }

    fn get_bonsai_id_from_bookmark(
        &self,
        ctx: CoreContext,
        bookmark: String,
    ) -> BoxFuture<ChangesetId, ErrorKind> {
        let bookmark_name = try_boxfuture!(BookmarkName::new(bookmark.clone()));
        match self.warm_bookmarks_cache.get(&bookmark_name) {
            Some(bookmark_value) => future::ok(bookmark_value).boxify(),
            None => self
                .repo
                .get_bonsai_bookmark(ctx, &bookmark_name)
                .from_err()
                .and_then(move |opt| {
                    opt.ok_or_else(|| ErrorKind::BookmarkNotFound(bookmark.to_string()))
                })
                .boxify(),
        }
    }

    fn get_unode_entry_by_changeset_id(
        &self,
        ctx: CoreContext,
        bcs_id: ChangesetId,
        path: Option<MPath>,
    ) -> BoxFuture<ManifestEntry<ManifestUnodeId, FileUnodeId>, ErrorKind> {
        cloned!(ctx, self.repo, self.unodes_derived_mapping);

        let blobstore = repo.get_blobstore();
        RootUnodeManifestId::derive(ctx.clone(), repo, unodes_derived_mapping, bcs_id)
            .map_err(ErrorKind::InternalError)
            .and_then({
                cloned!(blobstore, ctx, path);
                move |root_unode_mf_id| {
                    root_unode_mf_id
                        .manifest_unode_id()
                        .find_entry(ctx, blobstore, path)
                        .map_err(ErrorKind::InternalError)
                }
            })
            .and_then(move |maybe_entry| {
                maybe_entry.ok_or(ErrorKind::NotFound(
                    format!("{:?} {:?}", bcs_id, path),
                    None,
                ))
            })
            .boxify()
    }

    fn get_unode_entry(
        &self,
        ctx: CoreContext,
        revision: Revision,
        path: String,
    ) -> BoxFuture<ManifestEntry<ManifestUnodeId, FileUnodeId>, ErrorKind> {
        let mpath = if path.is_empty() {
            None
        } else {
            Some(try_boxfuture!(FS::get_mpath(path)))
        };

        cloned!(ctx);
        self.get_bonsai_id_from_revision(ctx.clone(), revision)
            .and_then({
                cloned!(ctx);
                let this = self.clone();
                move |bcs_id| this.get_unode_entry_by_changeset_id(ctx.clone(), bcs_id, mpath)
            })
            .boxify()
    }

    fn get_unode_changeset_id(
        &self,
        ctx: CoreContext,
        unode_entry: ManifestEntry<ManifestUnodeId, FileUnodeId>,
    ) -> BoxFuture<ChangesetId, ErrorKind> {
        cloned!(ctx, self.repo);
        let blobstore = repo.get_blobstore();
        unode_entry
            .load(ctx.clone(), &blobstore)
            .map_err(Error::from)
            .from_err()
            .map(move |unode| match unode {
                ManifestEntry::Tree(mf_unode) => mf_unode.linknode().clone(),
                ManifestEntry::Leaf(file_unode) => file_unode.linknode().clone(),
            })
            .boxify()
    }

    fn do_get_last_commit_on_path(
        &self,
        ctx: CoreContext,
        revision: Revision,
        path: String,
    ) -> BoxFuture<HgBlobChangeset, ErrorKind> {
        cloned!(ctx, self.repo);
        self.get_unode_entry(ctx.clone(), revision, path)
            .and_then({
                cloned!(ctx);
                let this = self.clone();
                move |unode_entry| this.get_unode_changeset_id(ctx.clone(), unode_entry)
            })
            .and_then({
                cloned!(ctx, repo);
                move |changeset_id| {
                    repo.get_hg_from_bonsai_changeset(ctx.clone(), changeset_id)
                        .from_err()
                }
            })
            .and_then(move |hg_changeset_id| {
                repo.get_changeset_by_changesetid(ctx, hg_changeset_id)
                    .from_err()
            })
            .boxify()
    }

    fn get_raw_file(
        &self,
        ctx: CoreContext,
        revision: Revision,
        path: String,
    ) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        STATS::get_raw_file.add_value(1);
        let mpath = try_boxfuture!(FS::get_mpath(path.clone()));

        let repo = self.repo.clone();
        self.get_hgchangesetid_from_revision(ctx.clone(), revision)
            .and_then({
                cloned!(repo, ctx);
                |changesetid| mononoke_api::get_content_by_path(ctx, repo, changesetid, Some(mpath))
            })
            .and_then({
                move |content| match content {
                    Content::File(stream)
                    | Content::Executable(stream)
                    | Content::Symlink(stream) => stream
                        .into_filestream()
                        .map(MononokeRepoResponse::GetRawFile)
                        .left_future(),
                    _ => Err(ErrorKind::InvalidInput(path.to_string(), None).into())
                        .into_future()
                        .right_future(),
                }
            })
            .from_err()
            .boxify()
    }

    fn is_ancestor(
        &self,
        ctx: CoreContext,
        ancestor: Revision,
        descendant: Revision,
    ) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        STATS::is_ancestor.add_value(1);
        let descendant_future = self.get_bonsai_id_from_revision(ctx.clone(), descendant.clone());
        let ancestor_future = self.get_bonsai_id_from_revision(ctx.clone(), ancestor.clone());

        descendant_future
            .join(ancestor_future)
            .map({
                cloned!(self.repo, self.skiplist_index);
                move |(desc, anc)| {
                    skiplist_index.query_reachability(ctx, repo.get_changeset_fetcher(), desc, anc)
                }
            })
            .flatten()
            .map(|answer| MononokeRepoResponse::IsAncestor { answer })
            .from_err()
            .boxify()
    }

    fn get_blob_content(
        &self,
        ctx: CoreContext,
        hash: String,
    ) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        STATS::get_blob_content.add_value(1);
        let blobhash = try_boxfuture!(FS::get_nodehash(&hash));

        self.repo
            .get_file_content(ctx, HgFileNodeId::new(blobhash))
            .into_filestream()
            .map(MononokeRepoResponse::GetBlobContent)
            .from_err()
            .boxify()
    }

    // TODO(aida): move it to the blobrepo
    fn get_hg_changeset_ids_by_bonsais(
        &self,
        ctx: CoreContext,
        changeset_ids: Vec<ChangesetId>,
    ) -> BoxFuture<Vec<HgChangesetId>, ErrorKind> {
        cloned!(ctx, self.repo);
        repo.get_hg_bonsai_mapping(ctx.clone(), changeset_ids.clone())
            .from_err()
            .and_then({
                cloned!(ctx, repo);
                move |hg_bonsai_list| {
                    let mapping: HashMap<_, _> = hg_bonsai_list
                        .into_iter()
                        .map(|(hg_id, bcs_id)| (bcs_id, hg_id))
                        .collect();

                    futures_ordered(changeset_ids.into_iter().map(|bcs_id| {
                        match mapping.get(&bcs_id) {
                            Some(hg_cs_id) => ok(*hg_cs_id).left_future(),
                            None => repo
                                .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id)
                                .map_err(ErrorKind::InternalError)
                                .right_future(),
                        }
                    }))
                    .collect()
                }
            })
            .boxify()
    }

    fn get_hg_changesets_by_ids(
        &self,
        ctx: CoreContext,
        changeset_ids: Vec<HgChangesetId>,
    ) -> BoxFuture<Vec<HgBlobChangeset>, ErrorKind> {
        let mut cs_futs = vec![];
        for cs_id in changeset_ids.into_iter() {
            cloned!(ctx, self.repo);
            cs_futs.push(
                repo.get_changeset_by_changesetid(ctx.clone(), cs_id)
                    .from_err(),
            );
        }
        futures_ordered(cs_futs).collect().boxify()
    }

    fn prefetch_history_batch(
        &self,
        ctx: CoreContext,
        changeset_id: ChangesetId,
        path: Option<MPath>,
    ) -> BoxFuture<Vec<(ChangesetId, Vec<FastlogParent>)>, ErrorKind> {
        cloned!(ctx, self.repo);
        self.get_unode_entry_by_changeset_id(ctx.clone(), changeset_id, path.clone())
            .and_then({
                cloned!(ctx, repo, path);
                move |entry| {
                    // optimistically try to fetch history for a unode
                    prefetch_history(ctx.clone(), repo.clone(), entry)
                        .map_err(Error::from)
                        .from_err()
                        .and_then({
                            move |maybe_history| match maybe_history {
                                Some(history) => ok(history).left_future(),
                                // if there is no history, let's try to derive batched fastlog data
                                // and fetch history again
                                None => {
                                    let fastlog_derived_mapping = Arc::new(
                                        RootFastlogMapping::new(Arc::new(repo.get_blobstore())),
                                    );
                                    RootFastlog::derive(
                                        ctx.clone(),
                                        repo.clone(),
                                        fastlog_derived_mapping,
                                        changeset_id,
                                    )
                                    .map_err(ErrorKind::InternalError)
                                    .and_then({
                                        cloned!(ctx, repo);
                                        move |_| {
                                            prefetch_history(ctx.clone(), repo.clone(), entry)
                                                .map_err(Error::from)
                                                .from_err()
                                        }
                                    })
                                    .and_then(move |maybe_history| {
                                        maybe_history.ok_or(ErrorKind::NotFound(
                                            format!("{:?} {:?}", changeset_id, path),
                                            None,
                                        ))
                                    })
                                    .right_future()
                                }
                            }
                        })
                }
            })
            .boxify()
    }

    fn do_history_graph_unfold(
        &self,
        ctx: CoreContext,
        changeset_id: ChangesetId,
        stage: i32,
        path: Option<MPath>,
        total_length: usize,
        history_graph: Arc<Mutex<HashMap<ChangesetId, Option<Vec<ChangesetId>>>>>,
        global_stage: Arc<Mutex<i32>>,
    ) -> BoxFuture<((), Vec<(ChangesetId, i32)>), ErrorKind> {
        cloned!(ctx);

        self.prefetch_history_batch(ctx.clone(), changeset_id, path.clone())
            .map({
                // construct the history graph
                move |history_batch: Vec<_>| {
                    let mut next = vec![];
                    let mut graph = history_graph.lock().unwrap();
                    for (cs_id, parents) in history_batch {
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
                                // the changeset hasn't been visited before
                                next.push((cs_id, stage + 1));
                            } else {
                                graph.insert(cs_id, Some(known_parents.clone()));
                            }
                        }
                    }

                    // We need staging so we would fetch all unode batches on the same depth level.
                    // For example, we need to return 120 history commits, but the fetched batch
                    // has only 110 and 5 changesets with unknown parents. Then on next iteration
                    // we need to fetch batches for _all_ these 5 changesets, so the bfs ordering
                    // in the end would be correct.
                    let mut global_stage = global_stage.lock().unwrap();
                    if graph.len() < total_length || *global_stage > stage {
                        // need to fetch more history
                        if *global_stage < stage + 1 {
                            *global_stage = stage + 1;
                        }
                        ((), next)
                    } else {
                        ((), vec![])
                    }
                }
            })
            .boxify()
    }

    fn sort_history(
        &self,
        changeset_id: ChangesetId,
        history_graph: &HashMap<ChangesetId, Option<Vec<ChangesetId>>>,
    ) -> Vec<ChangesetId> {
        let mut sorted = vec![changeset_id.clone()];
        let mut visited = HashSet::new();
        visited.insert(changeset_id);

        let mut next: usize = 0;
        while next < sorted.len() {
            if let Some(maybe_parents) = history_graph.get(&sorted[next]) {
                if let Some(parents) = maybe_parents {
                    for parent in parents {
                        if !visited.contains(parent) {
                            sorted.push(parent.clone());
                            visited.insert(*parent);
                        }
                    }
                }
            }
            next += 1;
        }
        return sorted;
    }

    fn get_file_history(
        &self,
        ctx: CoreContext,
        revision: Revision,
        path: String,
        limit: i32,
        skip: i32,
    ) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        STATS::get_file_history.add_value(1);

        /* validation */

        if limit <= 0 || skip < 0 {
            return future::err(ErrorKind::InvalidInput(
                format!("invalid parameters: limit {}, skip {}", limit, skip),
                None,
            ))
            .boxify();
        }

        let limit = limit as usize;
        let skip = skip as usize;

        // it's not necessary to fetch history in this case, we need just the most recent commit
        if skip == 0 && limit == 1 {
            return self
                .do_get_last_commit_on_path(ctx.clone(), revision, path)
                .and_then(move |changeset| {
                    changeset
                        .try_into()
                        .map_err(Error::from)
                        .map_err(ErrorKind::from)
                })
                .map(move |changeset| MononokeRepoResponse::GetFileHistory {
                    history: vec![changeset],
                })
                .boxify();
        }

        let mpath = if path.is_empty() {
            None
        } else {
            Some(try_boxfuture!(FS::get_mpath(path.clone())))
        };

        cloned!(ctx);
        let global_stage = Arc::new(Mutex::new(0));
        let history_graph: Arc<Mutex<HashMap<ChangesetId, Option<Vec<ChangesetId>>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        self.get_bonsai_id_from_revision(ctx.clone(), revision.clone())
            .and_then({
                // construct history graph and get the first changeset
                cloned!(ctx, global_stage, history_graph, mpath, skip, limit);
                let this = self.clone();
                move |bcs_id| {
                    bounded_traversal_dag(
                        256,
                        (bcs_id.clone(), 0),
                        // unfold
                        {
                            cloned!(ctx, mpath);
                            let this = this.clone();
                            move |(changeset_id, stage)| {
                                this.do_history_graph_unfold(
                                    ctx.clone(),
                                    changeset_id,
                                    stage,
                                    mpath.clone(),
                                    skip + limit,
                                    history_graph.clone(),
                                    global_stage.clone(),
                                )
                            }
                        },
                        // fold
                        move |_, _| ok(()),
                    )
                    .join(
                        this.get_unode_entry_by_changeset_id(ctx.clone(), bcs_id, mpath.clone())
                            .and_then({
                                let this = this.clone();
                                move |entry| this.get_unode_changeset_id(ctx.clone(), entry)
                            }),
                    )
                }
            })
            .and_then({
                cloned!(ctx);
                let this = self.clone();
                move |(_, changeset_id)| {
                    let history = this.sort_history(changeset_id, &history_graph.lock().unwrap());
                    let length = history.len();
                    let range = cmp::min(length, skip + limit);
                    let history_chunk = if skip > length {
                        vec![]
                    } else {
                        history[skip..range].to_vec()
                    };
                    this.get_hg_changeset_ids_by_bonsais(ctx.clone(), history_chunk)
                }
            })
            .and_then({
                let this = self.clone();
                move |hg_changeset_ids| this.get_hg_changesets_by_ids(ctx.clone(), hg_changeset_ids)
            })
            .and_then(move |changesets| {
                let maybe_result: Result<Vec<_>, _> = changesets
                    .into_iter()
                    .map(|changeset| {
                        changeset
                            .try_into()
                            .map_err(Error::from)
                            .map_err(ErrorKind::from)
                    })
                    .collect();
                maybe_result
            })
            .map(move |result| MononokeRepoResponse::GetFileHistory { history: result })
            .boxify()
    }

    fn get_last_commit_on_path(
        &self,
        ctx: CoreContext,
        revision: Revision,
        path: String,
    ) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        STATS::get_last_commit_on_path.add_value(1);

        self.do_get_last_commit_on_path(ctx.clone(), revision, path)
            .and_then(move |changeset| {
                changeset
                    .try_into()
                    .map_err(Error::from)
                    .map_err(ErrorKind::from)
            })
            .map(move |changeset| MononokeRepoResponse::GetLastCommitOnPath { commit: changeset })
            .boxify()
    }

    fn list_directory(
        &self,
        ctx: CoreContext,
        revision: Revision,
        path: String,
    ) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        STATS::list_directory.add_value(1);
        let mpath = if path.is_empty() {
            None
        } else {
            Some(try_boxfuture!(FS::get_mpath(path.clone())))
        };

        let repo = self.repo.clone();
        self.get_hgchangesetid_from_revision(ctx.clone(), revision)
            .and_then(move |changesetid| {
                mononoke_api::get_content_by_path(ctx, repo, changesetid, mpath)
            })
            .and_then(move |content| match content {
                Content::Tree(tree) => Ok(tree),
                _ => Err(ErrorKind::NotADirectory(path.to_string()).into()),
            })
            .map(|tree| {
                tree.list()
                    .filter_map(|entry| -> Option<Entry> { entry.try_into().ok() })
            })
            .map(|files| MononokeRepoResponse::ListDirectory {
                files: files.collect(),
            })
            .from_err()
            .boxify()
    }

    fn list_directory_unodes(
        &self,
        ctx: CoreContext,
        revision: Revision,
        path: String,
    ) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        STATS::list_directory_unodes.add_value(1);

        cloned!(ctx, self.repo);
        let blobstore = repo.get_blobstore();
        self.get_unode_entry(ctx.clone(), revision, path.clone())
            .and_then({
                cloned!(blobstore, ctx);
                move |entry| match entry {
                    ManifestEntry::Tree(tree) => tree
                        .load(ctx, &blobstore)
                        .map_err(Error::from)
                        .from_err()
                        .left_future(),
                    ManifestEntry::Leaf(_) => err(ErrorKind::InvalidInput(
                        format!("{} is not a directory", path),
                        None,
                    ))
                    .right_future(),
                }
            })
            .and_then(|unode_mf| {
                let res: Result<Vec<_>, _> = unode_mf
                    .list()
                    .map(|(name, entry)| {
                        String::from_utf8(name.to_bytes().to_vec())
                            .map(|name| EntryLight {
                                name,
                                is_directory: entry.is_directory(),
                            })
                            .map_err(|err| {
                                ErrorKind::InvalidInput(
                                    "non utf8 path".to_string(),
                                    Some(Error::from(err)),
                                )
                            })
                    })
                    .collect();
                res
            })
            .map(|entries| MononokeRepoResponse::ListDirectoryUnodes { files: entries })
            .boxify()
    }

    fn get_tree(
        &self,
        ctx: CoreContext,
        hash: String,
    ) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        STATS::get_tree.add_value(1);
        let treehash = try_boxfuture!(FS::get_nodehash(&hash));
        let treemanifestid = HgManifestId::new(treehash);

        self.repo
            .get_manifest_by_nodeid(ctx.clone(), treemanifestid)
            .map({
                cloned!(self.cache, self.repo);
                move |tree| {
                    join_all(tree.list().map(move |entry| {
                        EntryWithSizeAndContentHash::materialize_future(
                            ctx.clone(),
                            repo.clone(),
                            entry,
                            cache.clone(),
                        )
                    }))
                }
            })
            .flatten()
            .map(|files| MononokeRepoResponse::GetTree { files })
            .from_err()
            .boxify()
    }

    fn get_changeset(
        &self,
        ctx: CoreContext,
        revision: Revision,
    ) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        STATS::get_changeset.add_value(1);
        let repo = self.repo.clone();
        self.get_hgchangesetid_from_revision(ctx.clone(), revision)
            .and_then(move |changesetid| repo.get_changeset_by_changesetid(ctx, changesetid))
            .and_then(|changeset| changeset.try_into().map_err(From::from))
            .map(|changeset| MononokeRepoResponse::GetChangeset { changeset })
            .from_err()
            .boxify()
    }

    fn get_branches(&self, ctx: CoreContext) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        STATS::get_branches.add_value(1);
        let bookmarks = self.warm_bookmarks_cache.get_all();
        let mut futs = FuturesUnordered::new();
        for (key, value) in bookmarks.into_iter() {
            let key = key.clone();
            futs.push(
                self.repo
                    .get_hg_from_bonsai_changeset(ctx.clone(), value.clone())
                    .map(move |hg_cs_id| (key.to_string(), hg_cs_id.to_hex().to_string()))
                    .from_err(),
            );
        }

        futs.collect_to::<BTreeMap<_, _>>()
            .map(|branches| MononokeRepoResponse::GetBranches { branches })
            .boxify()
    }

    fn eden_get_data(
        &self,
        ctx: CoreContext,
        keys: Vec<Key>,
        stream: bool,
    ) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        STATS::eden_get_data.add_value(1);
        let mut fetches = Vec::new();
        for key in keys {
            let filenode = HgFileNodeId::new(key.hgid.clone().into());
            let get_parents = self.repo.get_file_parents(ctx.clone(), filenode);

            let get_content =
                create_getpack_v1_blob(ctx.clone(), self.repo.clone(), filenode.clone(), false)
                    .and_then(|(_size, fut)| fut)
                    .map(|(_filenode, bytes)| bytes);

            // Use `lazy` when writing log messages so that the message is emitted
            // when the Future is polled rather than when it is created.
            let logger = self.logger.clone();
            let fut = lazy(move || {
                debug!(&logger, "fetching data for key: {}", &key);

                get_parents.and_then(move |parents| {
                    get_content.map(move |bytes| DataEntry::new(key, bytes, parents.into()))
                })
            });

            fetches.push(fut);
        }

        let entries = iter_ok(fetches).buffer_unordered(10);
        if stream {
            ok(MononokeRepoResponse::EdenGetDataStream(entries.boxify())).boxify()
        } else {
            entries
                .collect()
                .map(|entries| MononokeRepoResponse::EdenGetData(DataResponse::new(entries)))
                .from_err()
                .boxify()
        }
    }

    fn eden_get_history(
        &self,
        ctx: CoreContext,
        keys: Vec<Key>,
        depth: Option<u32>,
        stream: bool,
    ) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        STATS::eden_get_history.add_value(1);
        let mut fetches = Vec::new();
        for key in keys {
            let ctx = ctx.clone();
            let repo = self.repo.clone();
            let filenode = HgFileNodeId::new(key.hgid.clone().into());
            let logger = self.logger.clone();

            let fut = MPath::new(key.path.as_byte_slice())
                .into_future()
                .and_then(move |path| {
                    debug!(&logger, "fetching history for key: {}", &key);
                    get_file_history(ctx, repo, filenode, path, depth)
                        .and_then(move |entry| {
                            let entry = WireHistoryEntry::try_from(entry)?;
                            Ok((key.path.clone(), entry))
                        })
                        .collect()
                        .from_err()
                });

            fetches.push(fut);
        }

        let entries = iter_ok(fetches).buffer_unordered(10).map(iter_ok).flatten();
        if stream {
            ok(MononokeRepoResponse::EdenGetHistoryStream(entries.boxify())).boxify()
        } else {
            entries
                .collect()
                .map(|entries| MononokeRepoResponse::EdenGetHistory(HistoryResponse::new(entries)))
                .from_err()
                .boxify()
        }
    }

    fn eden_get_trees(
        &self,
        ctx: CoreContext,
        keys: Vec<Key>,
        stream: bool,
    ) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        STATS::eden_get_trees.add_value(1);
        let mut fetches = Vec::new();
        for key in keys {
            let manifest_id = HgManifestId::new(key.hgid.clone().into());
            let entry = self.repo.get_root_entry(manifest_id);
            let get_parents = entry.get_parents(ctx.clone());
            let get_content = entry.get_raw_content(ctx.clone());

            // Use `lazy` when writing log messages so that the message is emitted
            // when the Future is polled rather than when it is created.
            let logger = self.logger.clone();
            let fut = lazy(move || {
                debug!(&logger, "fetching tree for key: {}", &key);

                get_parents.and_then(move |parents| {
                    get_content.map(move |content| {
                        DataEntry::new(key, content.into_inner(), parents.into())
                    })
                })
            });

            fetches.push(fut);
        }

        let entries = iter_ok(fetches).buffer_unordered(10);
        if stream {
            ok(MononokeRepoResponse::EdenGetTreesStream(entries.boxify())).boxify()
        } else {
            entries
                .collect()
                .map(|entries| MononokeRepoResponse::EdenGetTrees(DataResponse::new(entries)))
                .from_err()
                .boxify()
        }
    }

    fn eden_prefetch_trees(
        &self,
        ctx: CoreContext,
        req: TreeRequest,
        stream: bool,
    ) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        STATS::eden_prefetch_trees.add_value(1);
        let entries = gettreepack_entries(ctx.clone(), &self.repo, req.into()).and_then(
            move |(entry, basepath)| {
                let full_path = MPath::join_element_opt(basepath.as_ref(), entry.get_name());
                let path_bytes = full_path
                    .map(|mpath| mpath.to_vec())
                    .unwrap_or_else(Vec::new);
                let path = try_boxfuture!(RepoPathBuf::from_utf8(path_bytes));

                let node = entry.get_hash().into_nodehash().into();
                let key = Key::new(path, node);

                let get_parents = entry.get_parents(ctx.clone());
                let get_content = entry.get_raw_content(ctx.clone());
                get_parents
                    .and_then(move |parents| {
                        get_content.map(move |content| {
                            DataEntry::new(key, content.into_inner(), parents.into())
                        })
                    })
                    .boxify()
            },
        );

        if stream {
            ok(MononokeRepoResponse::EdenPrefetchTreesStream(
                entries.boxify(),
            ))
            .boxify()
        } else {
            entries
                .collect()
                .map(|entries| MononokeRepoResponse::EdenPrefetchTrees(DataResponse::new(entries)))
                .from_err()
                .boxify()
        }
    }

    pub fn send_query(
        &self,
        ctx: CoreContext,
        msg: MononokeRepoQuery,
    ) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        use crate::MononokeRepoQuery::*;

        let context = ctx.clone();
        let query = serde_json::to_value(&msg).unwrap_or(serde_json::json!(null));

        let query_fut = match msg {
            GetRawFile { revision, path } => self.get_raw_file(ctx, revision, path),
            GetBlobContent { hash } => self.get_blob_content(ctx, hash),
            ListDirectory { revision, path } => self.list_directory(ctx, revision, path),
            ListDirectoryUnodes { revision, path } => {
                self.list_directory_unodes(ctx, revision, path)
            }
            GetTree { hash } => self.get_tree(ctx, hash),
            GetChangeset { revision } => self.get_changeset(ctx, revision),
            GetBranches => self.get_branches(ctx),
            GetFileHistory {
                revision,
                path,
                limit,
                skip,
            } => self.get_file_history(ctx, revision, path, limit, skip),
            GetLastCommitOnPath { revision, path } => {
                self.get_last_commit_on_path(ctx, revision, path)
            }
            IsAncestor {
                ancestor,
                descendant,
            } => self.is_ancestor(ctx, ancestor, descendant),
            EdenGetData {
                request: DataRequest { keys },
                stream,
            } => self.eden_get_data(ctx, keys, stream),
            EdenGetHistory {
                request: HistoryRequest { keys, depth },
                stream,
            } => self.eden_get_history(ctx, keys, depth, stream),
            EdenGetTrees {
                request: DataRequest { keys },
                stream,
            } => self.eden_get_trees(ctx, keys, stream),
            EdenPrefetchTrees { request, stream } => self.eden_prefetch_trees(ctx, request, stream),
        };

        query_fut.timed({
            move |stats, resp| {
                log_result(&context, context.scuba().clone(), resp, &stats, &query);

                Ok(())
            }
        })
    }
}

fn log_result(
    ctx: &CoreContext,
    mut scuba: ScubaSampleBuilder,
    resp: Result<&MononokeRepoResponse, &ErrorKind>,
    stats: &FutureStats,
    query: &serde_json::value::Value,
) {
    if !ctx.perf_counters().is_empty() {
        ctx.perf_counters().insert_perf_counters(&mut scuba);
    }

    let server_error = match resp {
        Ok(_) => false,
        Err(err) => err.is_server_error(),
    };

    scuba
        .add_future_stats(&stats)
        .add("response_time", stats.completion_time.as_micros_unchecked())
        .add(
            "params",
            query
                .get("params")
                .unwrap_or(&serde_json::json!("unknown"))
                .to_string(),
        )
        .add(
            "method",
            query
                .get("method")
                .unwrap_or(&serde_json::json!("unknown"))
                .to_string()
                .trim_matches('"'),
        )
        .add("log_tag", "Finished processing")
        .add("success", resp.is_ok())
        .add("server_error", server_error);

    scuba.log();
}
