// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

use super::HookResults;
use blobrepo::BlobRepo;
use bookmarks::BookmarkName;
use cloned::cloned;
use context::CoreContext;
use failure::{err_msg, Error, Result};
use futures::{Future, Stream};
use futures_ext::{spawn_future, BoxFuture, FutureExt};
use hooks::{hook_loader::load_hooks, HookManager};
use hooks_content_stores::{BlobRepoChangesetStore, BlobRepoFileContentStore};
use manifold::{ManifoldHttpClient, PayloadRange};
use mercurial_types::HgChangesetId;
use metaconfig_types::RepoConfig;
use mononoke_types::ChangesetId;
use revset::AncestorsNodeStream;
use slog::{debug, info, Logger};
use std::collections::HashSet;
use std::sync::Arc;

pub struct Tailer {
    ctx: CoreContext,
    repo: BlobRepo,
    hook_manager: Arc<HookManager>,
    bookmark: BookmarkName,
    last_rev_key: String,
    manifold_client: ManifoldHttpClient,
    logger: Logger,
    excludes: HashSet<ChangesetId>,
}

impl Tailer {
    pub fn new(
        ctx: CoreContext,
        repo: BlobRepo,
        config: RepoConfig,
        bookmark: BookmarkName,
        manifold_client: ManifoldHttpClient,
        logger: Logger,
        excludes: HashSet<ChangesetId>,
    ) -> Result<Tailer> {
        let changeset_store = BlobRepoChangesetStore::new(repo.clone());
        let content_store = BlobRepoFileContentStore::new(repo.clone());

        let mut hook_manager = HookManager::new(
            ctx.clone(),
            Box::new(changeset_store),
            Arc::new(content_store),
            Default::default(),
            logger.clone(),
        );

        load_hooks(&mut hook_manager, config)?;

        let repo_id = repo.get_repoid().id();
        let last_rev_key = format!("{}{}", "__mononoke_hook_tailer_last_rev.", repo_id).to_string();

        Ok(Tailer {
            ctx,
            repo,
            hook_manager: Arc::new(hook_manager),
            bookmark,
            last_rev_key,
            manifold_client,
            logger,
            excludes,
        })
    }

    pub fn get_last_rev_key(&self) -> String {
        self.last_rev_key.clone()
    }

    fn run_in_range0(
        ctx: CoreContext,
        repo: BlobRepo,
        hm: Arc<HookManager>,
        last_rev: HgChangesetId,
        end_rev: HgChangesetId,
        bm: BookmarkName,
        logger: Logger,
        excludes: HashSet<ChangesetId>,
    ) -> BoxFuture<Vec<HookResults>, Error> {
        debug!(logger, "Running in range {} to {}", last_rev, end_rev);
        cloned!(logger);
        nodehash_to_bonsai(ctx.clone(), &repo, end_rev)
            .and_then(move |end_rev| {
                AncestorsNodeStream::new(ctx.clone(), &repo.get_changeset_fetcher(), end_rev)
                    .take(1000) // Limit number so we don't process too many
                    .filter(move |cs| !excludes.contains(cs))
                    .map({
                        move |cs| {
                            cloned!(ctx, bm, hm, logger, repo);
                            run_hooks_for_changeset(ctx, repo, hm, bm, cs, logger)
                        }
                    })
                    .map(spawn_future)
                    .buffered(100)
                    .take_while(move |(hg_cs, _)| {
                        Ok(*hg_cs != last_rev)
                    })
                    .map(|(_, res)| res)
                    .collect()
            })
            .boxify()
    }

    pub fn run_in_range(
        &self,
        last_rev: HgChangesetId,
        end_rev: HgChangesetId,
    ) -> BoxFuture<Vec<HookResults>, Error> {
        cloned!(
            self.ctx,
            self.repo,
            self.hook_manager,
            self.bookmark,
            self.excludes
        );
        Tailer::run_in_range0(
            ctx,
            repo,
            hook_manager,
            last_rev,
            end_rev,
            bookmark,
            self.logger.clone(),
            excludes,
        )
    }

    pub fn run_single_changeset(
        &self,
        changeset: HgChangesetId,
    ) -> BoxFuture<Vec<HookResults>, Error> {
        cloned!(
            self.ctx,
            self.repo,
            self.hook_manager,
            self.bookmark,
            self.logger
        );
        repo.get_bonsai_from_hg(ctx, changeset)
            .and_then(move |maybe_bonsai| {
                maybe_bonsai.ok_or(err_msg(format!(
                    "changeset does not exist {}",
                    changeset.to_string()
                )))
            })
            .and_then({
                cloned!(self.ctx);
                move |bonsai| {
                    run_hooks_for_changeset(ctx, repo, hook_manager, bookmark, bonsai, logger)
                }
            })
            .map(|(_, result)| vec![result])
            .boxify()
    }

    pub fn run_with_limit(&self, limit: u64) -> BoxFuture<Vec<HookResults>, Error> {
        let ctx = self.ctx.clone();
        let bm = self.bookmark.clone();
        let hm = self.hook_manager.clone();
        let excludes = self.excludes.clone();

        let bm_rev = self
            .repo
            .get_bookmark(ctx.clone(), &bm)
            .and_then({
                cloned!(bm);
                |opt| opt.ok_or(ErrorKind::NoSuchBookmark(bm).into())
            })
            .and_then({
                cloned!(ctx, self.repo);
                move |bm_rev| nodehash_to_bonsai(ctx, &repo, bm_rev)
            });

        cloned!(self.ctx, self.repo, self.logger);
        bm_rev
            .and_then(move |bm_rev| {
                AncestorsNodeStream::new(ctx.clone(), &repo.get_changeset_fetcher(), bm_rev)
                    .take(limit)
                    .filter(move |cs| !excludes.contains(cs))
                    .map({
                        move |cs| {
                            cloned!(ctx, bm, hm, logger, repo);
                            run_hooks_for_changeset(ctx, repo, hm, bm, cs, logger)
                        }
                    })
                    .map(spawn_future)
                    .buffered(100)
                    .map(|(_, res)| res)
                    .collect()
            })
            .boxify()
    }

    pub fn run(&self) -> BoxFuture<Vec<HookResults>, Error> {
        info!(
            self.logger,
            "Running tailer on bookmark {}",
            self.bookmark.clone()
        );

        self.repo
            .get_bookmark(self.ctx.clone(), &self.bookmark.clone())
            .and_then({
                cloned!(self.bookmark);
                |opt| opt.ok_or(ErrorKind::NoSuchBookmark(bookmark).into())
            })
            .and_then({
                cloned!(self.last_rev_key, self.manifold_client);
                move |current_bm_cs| {
                    manifold_client
                        .read(last_rev_key, PayloadRange::Full)
                        .map(move |opt| (current_bm_cs, opt))
                }
            })
            .and_then(|(current_bm_cs, opt)| match opt {
                Some(last_rev_bytes) => Ok((current_bm_cs, last_rev_bytes)),
                None => Err(ErrorKind::NoLastRevision.into()),
            })
            .and_then(|(current_bm_cs, last_rev_bytes)| {
                let node_hash = HgChangesetId::from_bytes(&*last_rev_bytes.payload.payload)?;
                Ok((current_bm_cs, node_hash))
            })
            .and_then({
                cloned!(
                    self.logger,
                    self.bookmark,
                    self.excludes,
                    self.hook_manager,
                    self.repo,
                    self.ctx
                );
                move |(current_bm_cs, last_rev)| {
                    let end_rev = current_bm_cs;
                    info!(
                        logger,
                        "Bookmark is currently at {}, last processed revision is {}",
                        end_rev,
                        last_rev
                    );
                    if last_rev == end_rev {
                        info!(logger, "Nothing to do");
                    }
                    Tailer::run_in_range0(
                        ctx,
                        repo,
                        hook_manager,
                        last_rev,
                        end_rev,
                        bookmark,
                        logger,
                        excludes,
                    )
                    .map(move |res| (end_rev, res))
                }
            })
            .and_then({
                cloned!(self.last_rev_key, self.logger, self.manifold_client);
                move |(end_rev, res)| {
                    info!(logger, "Setting last processed revision to {:?}", end_rev);
                    let bytes = end_rev.as_bytes().into();
                    manifold_client.write(last_rev_key, bytes).map(|()| res)
                }
            })
            .boxify()
    }
}

fn nodehash_to_bonsai(
    ctx: CoreContext,
    repo: &BlobRepo,
    hg_cs: HgChangesetId,
) -> impl Future<Item = ChangesetId, Error = Error> {
    repo.get_bonsai_from_hg(ctx, hg_cs)
        .and_then(move |maybe_node| maybe_node.ok_or(ErrorKind::BonsaiNotFound(hg_cs).into()))
}

fn run_hooks_for_changeset(
    ctx: CoreContext,
    repo: BlobRepo,
    hm: Arc<HookManager>,
    bm: BookmarkName,
    cs: ChangesetId,
    logger: Logger,
) -> impl Future<Item = (HgChangesetId, HookResults), Error = Error> {
    repo.get_hg_from_bonsai_changeset(ctx.clone(), cs)
        .and_then(move |hg_cs| {
            debug!(logger, "Running file hooks for changeset {:?}", hg_cs);
            hm.run_file_hooks_for_bookmark(ctx.clone(), hg_cs.clone(), &bm, None)
                .map(move |res| (hg_cs, res))
                .and_then(move |(hg_cs, file_res)| {
                    let hg_cs = hg_cs.clone();
                    debug!(logger, "Running changeset hooks for changeset {:?}", hg_cs);
                    hm.run_changeset_hooks_for_bookmark(ctx, hg_cs.clone(), &bm, None)
                        .map(move |res| {
                            let hook_results = HookResults {
                                file_hooks_results: file_res,
                                cs_hooks_result: res,
                            };
                            (hg_cs, hook_results)
                        })
                })
        })
}

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "No such bookmark '{}'", _0)]
    NoSuchBookmark(BookmarkName),
    #[fail(display = "Cannot find last revision in blobstore")]
    NoLastRevision,
    #[fail(display = "Cannot find bonsai for {}", _0)]
    BonsaiNotFound(HgChangesetId),
}
