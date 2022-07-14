/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Error;
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use bookmarks::BookmarkName;
use cloned::cloned;
use context::CoreContext;
use context::PerfCounterType;
use derived_data::BonsaiDerived;
use derived_data_filenodes::FilenodesOnlyPublic;
use filenodes::FilenodeResult;
use futures::compat::Stream01CompatExt;
use futures::future;
use futures::future::TryFutureExt;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures_stats::TimedFutureExt;
use manifest::Entry;
use manifest::ManifestOps;
use mercurial_derived_data::DeriveHgChangeset;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileNodeId;
use mercurial_types::RepoPath;
use metaconfig_types::CacheWarmupParams;
use microwave::SnapshotLocation;
use mononoke_types::ChangesetId;
use revset::AncestorsNodeStream;
use slog::debug;
use slog::info;
use slog::warn;
use tokio::task;

mod errors {
    use bookmarks::BookmarkName;
    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum ErrorKind {
        #[error("Bookmark {0} does not exist")]
        BookmarkNotFound(BookmarkName),
    }
}

#[derive(Debug)]
pub enum CacheWarmupTarget {
    Bookmark(BookmarkName),
    Changeset(ChangesetId),
}

#[derive(Debug)]
pub struct CacheWarmupRequest {
    pub target: CacheWarmupTarget,
    pub commit_limit: usize,
    pub microwave_preload: bool,
}

impl From<CacheWarmupParams> for CacheWarmupRequest {
    fn from(other: CacheWarmupParams) -> Self {
        let CacheWarmupParams {
            bookmark,
            commit_limit,
            microwave_preload,
        } = other;

        Self {
            target: CacheWarmupTarget::Bookmark(bookmark),
            commit_limit,
            microwave_preload,
        }
    }
}

// Fetches all the manifest entries and their linknodes. Do not fetching files because
// there can be too many of them.
async fn blobstore_and_filenodes_warmup(
    ctx: &CoreContext,
    repo: &BlobRepo,
    bcs_id: ChangesetId,
    hg_cs_id: HgChangesetId,
) -> Result<(), Error> {
    // TODO(stash): Arbitrary number. Tweak somehow?
    let buffer_size = 100usize;

    // Ensure filenodes are derived for this, and load the changeset.
    let (cs, ()) = future::try_join(
        hg_cs_id.load(ctx, repo.blobstore()).map_err(Error::from),
        FilenodesOnlyPublic::derive(ctx, repo, bcs_id)
            .map_err(Error::from)
            .map_ok(|_| ()),
    )
    .await?;

    let null_linknodes = cs
        .manifestid()
        .list_all_entries(ctx.clone(), repo.get_blobstore())
        .try_filter_map(|(path, entry)| {
            let item = match entry {
                Entry::Leaf(_) => None,
                Entry::Tree(hash) => {
                    let hash = HgFileNodeId::new(hash.into_nodehash());

                    let path = match path {
                        Some(path) => RepoPath::DirectoryPath(path),
                        None => RepoPath::RootPath,
                    };

                    cloned!(ctx, repo);
                    let fut = async move {
                        let filenode_res = repo.get_filenode_opt(ctx, &path, hash).await?;
                        let linknode_res = filenode_res
                            .map(|filenode_opt| filenode_opt.map(|filenode| filenode.linknode));
                        match linknode_res {
                            FilenodeResult::Present(maybe_linknode) => Ok(maybe_linknode),
                            FilenodeResult::Disabled => Ok(None),
                        }
                    };

                    Some(fut)
                }
            };

            future::ready(Ok(item))
        })
        .try_buffer_unordered(buffer_size)
        .try_fold(0u64, {
            let mut i = 0;
            move |mut null_linknodes, linknode| {
                if linknode.is_none() {
                    ctx.perf_counters()
                        .increment_counter(PerfCounterType::NullLinknode);
                    null_linknodes += 1;
                }

                i += 1;
                if i % 10000 == 0 {
                    debug!(ctx.logger(), "manifests warmup: fetched {}th entry", i);
                }

                future::ready(Result::<_, Error>::Ok(null_linknodes))
            }
        })
        .await?;

    if null_linknodes > 0 {
        warn!(ctx.logger(), "{} linknodes are missing!", null_linknodes);
    }
    debug!(ctx.logger(), "finished manifests warmup");

    Ok(())
}

// Iterate over first parents, and fetch them
async fn changesets_warmup(
    ctx: &CoreContext,
    repo: &BlobRepo,
    bcs_id: ChangesetId,
    cs_limit: usize,
) -> Result<(), Error> {
    info!(ctx.logger(), "about to start warming up changesets cache");

    AncestorsNodeStream::new(ctx.clone(), &repo.get_changeset_fetcher(), bcs_id)
        .compat()
        .take(cs_limit)
        .try_for_each({
            let mut i = 0;
            move |_: ChangesetId| {
                i += 1;
                if i % 10000 == 0 {
                    debug!(ctx.logger(), "changesets warmup: fetched {}th entry", i);
                }
                future::ready(Ok(()))
            }
        })
        .await?;

    debug!(ctx.logger(), "finished changesets warmup");

    Ok(())
}

async fn do_cache_warmup(
    ctx: &CoreContext,
    repo: &BlobRepo,
    target: CacheWarmupTarget,
    commit_limit: usize,
) -> Result<(), Error> {
    let ctx = ctx.clone_and_reset();

    let bcs_id = match target {
        CacheWarmupTarget::Bookmark(bookmark) => repo
            .get_bonsai_bookmark(ctx.clone(), &bookmark)
            .await?
            .ok_or(errors::ErrorKind::BookmarkNotFound(bookmark))?,
        CacheWarmupTarget::Changeset(bcs_id) => bcs_id,
    };

    let hg_cs_id = repo.derive_hg_changeset(&ctx, bcs_id).await?;

    let blobstore_warmup = task::spawn({
        cloned!(ctx, repo);
        async move {
            blobstore_and_filenodes_warmup(&ctx, &repo, bcs_id, hg_cs_id)
                .await
                .context("While warming up blobstore and filenodes")
        }
    });

    let cs_warmup = task::spawn({
        cloned!(ctx, repo);
        async move {
            changesets_warmup(&ctx, &repo, bcs_id, commit_limit)
                .await
                .context("While warming up changesets")
        }
    });

    let (stats, res) = future::try_join(blobstore_warmup, cs_warmup).timed().await;
    let (blobstore_warmup, cs_warmup) = res?;
    blobstore_warmup?;
    cs_warmup?;

    info!(ctx.logger(), "finished initial warmup");

    let mut scuba = ctx.scuba().clone();
    scuba.add_future_stats(&stats);
    ctx.perf_counters().insert_perf_counters(&mut scuba);
    scuba.log_with_msg("Cache warmup complete", None);
    Ok(())
}

async fn microwave_preload(ctx: &CoreContext, repo: &BlobRepo, req: &CacheWarmupRequest) {
    if req.microwave_preload {
        match microwave::prime_cache(ctx, repo, SnapshotLocation::Blobstore).await {
            Ok(_) => {
                warn!(ctx.logger(), "microwave: successfully primed cache");
            }
            Err(e) => {
                warn!(ctx.logger(), "microwave: cache warmup failed: {:#?}", e);
            }
        }
    }
}

/// Fetch all manifest entries for a bookmark, and fetches up to `commit_warmup_limit`
/// ancestors of the bookmark.
pub async fn cache_warmup<T: Into<CacheWarmupRequest>>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cache_warmup: Option<T>,
) -> Result<(), Error> {
    if let Some(req) = cache_warmup {
        let req = req.into();

        microwave_preload(ctx, repo, &req).await;

        do_cache_warmup(ctx, repo, req.target, req.commit_limit)
            .await
            .with_context(|| format!("while warming up repo {}", repo.get_repoid()))?;
    }

    Ok(())
}
