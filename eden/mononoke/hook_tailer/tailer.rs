/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::{Error, Result};
use blobrepo::BlobRepo;
use blobstore::Loadable;
use bookmarks::BookmarkName;
use cloned::cloned;
use context::CoreContext;
use futures::{
    compat::{Future01CompatExt, Stream01CompatExt},
    future::{self, FutureExt, TryFutureExt},
    stream::{StreamExt, TryStreamExt},
};
use futures_ext::{BoxFuture, FutureExt as OldFutureExt};
use futures_old::Future as OldFuture;
use hooks::{hook_loader::load_hooks, HookManager, HookOutcome};
use hooks_content_stores::blobrepo_text_only_fetcher;
use mercurial_types::HgChangesetId;
use metaconfig_types::RepoConfig;
use mononoke_types::ChangesetId;
use revset::AncestorsNodeStream;
use scuba_ext::ScubaSampleBuilder;
use slog::debug;
use std::collections::HashSet;
use std::sync::Arc;
use thiserror::Error;
use tokio::task;

pub struct Tailer {
    ctx: CoreContext,
    repo: BlobRepo,
    hook_manager: Arc<HookManager>,
    bookmark: BookmarkName,
    excludes: HashSet<ChangesetId>,
}

impl Tailer {
    pub fn new(
        ctx: CoreContext,
        repo: BlobRepo,
        config: RepoConfig,
        bookmark: BookmarkName,
        excludes: HashSet<ChangesetId>,
        disabled_hooks: &HashSet<String>,
    ) -> Result<Tailer> {
        let content_fetcher = blobrepo_text_only_fetcher(repo.clone(), config.hook_max_file_size);

        let mut hook_manager = HookManager::new(
            ctx.fb,
            content_fetcher,
            Default::default(),
            ScubaSampleBuilder::with_discard(),
        );

        load_hooks(ctx.fb, &mut hook_manager, config, disabled_hooks)?;

        Ok(Tailer {
            ctx,
            repo,
            hook_manager: Arc::new(hook_manager),
            bookmark,
            excludes,
        })
    }

    pub fn run_single_changeset(
        &self,
        changeset: ChangesetId,
    ) -> BoxFuture<Vec<HookOutcome>, Error> {
        cloned!(self.ctx, self.repo, self.hook_manager, self.bookmark,);
        run_hooks_for_changeset(ctx, repo, hook_manager, bookmark, changeset)
            .map(|(_, result)| result)
            .boxify()
    }

    pub async fn run_with_limit(&self, limit: usize) -> Result<Vec<HookOutcome>, Error> {
        let bm_rev = self
            .repo
            .get_bonsai_bookmark(self.ctx.clone(), &self.bookmark)
            .compat()
            .await?
            .ok_or_else(|| ErrorKind::NoSuchBookmark(self.bookmark.clone()))?;

        let res =
            AncestorsNodeStream::new(self.ctx.clone(), &self.repo.get_changeset_fetcher(), bm_rev)
                .compat()
                .take(limit)
                .try_filter(|cs_id| future::ready(!self.excludes.contains(cs_id)))
                .map(|cs_id| async move {
                    match cs_id {
                        Ok(cs_id) => {
                            let (_, outcomes) = task::spawn(
                                run_hooks_for_changeset(
                                    self.ctx.clone(),
                                    self.repo.clone(),
                                    self.hook_manager.clone(),
                                    self.bookmark.clone(),
                                    cs_id,
                                )
                                .compat(),
                            )
                            .await??;

                            Ok(outcomes)
                        }
                        Err(e) => Err(e),
                    }
                })
                .buffered(100)
                .try_concat()
                .await?;

        Ok(res)
    }
}

fn run_hooks_for_changeset(
    ctx: CoreContext,
    repo: BlobRepo,
    hm: Arc<HookManager>,
    bm: BookmarkName,
    cs_id: ChangesetId,
) -> impl OldFuture<Item = (ChangesetId, Vec<HookOutcome>), Error = Error> {
    cs_id
        .load(ctx.clone(), repo.blobstore())
        .from_err()
        .and_then(move |cs| {
            async move {
                debug!(ctx.logger(), "Running hooks for changeset {:?}", cs);
                let hook_results = hm
                    .run_hooks_for_bookmark(&ctx, vec![cs].iter(), &bm, None)
                    .await?;
                Ok((cs_id, hook_results))
            }
            .boxed()
            .compat()
        })
}

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("No such bookmark '{0}'")]
    NoSuchBookmark(BookmarkName),
    #[error("Cannot find last revision in blobstore")]
    NoLastRevision,
    #[error("Cannot find bonsai for {0}")]
    BonsaiNotFound(HgChangesetId),
}
