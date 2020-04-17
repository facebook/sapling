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
    future::{self, TryFutureExt},
    stream::{Stream, StreamExt, TryStreamExt},
};
use futures_stats::{FutureStats, TimedFutureExt};
use hooks::{hook_loader::load_hooks, HookManager, HookOutcome};
use hooks_content_stores::blobrepo_text_only_fetcher;
use metaconfig_types::RepoConfig;
use mononoke_types::ChangesetId;
use revset::AncestorsNodeStream;
use scuba_ext::ScubaSampleBuilder;
use slog::debug;
use std::collections::HashSet;
use std::sync::Arc;
use thiserror::Error;
use tokio::task;

pub struct HookExecutionInstance {
    pub cs_id: ChangesetId,
    pub file_count: usize,
    pub stats: FutureStats,
    pub outcomes: Vec<HookOutcome>,
}

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
            config.hook_manager_params.clone().unwrap_or_default(),
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

    pub async fn run_single_changeset(
        &self,
        changeset: ChangesetId,
    ) -> Result<HookExecutionInstance, Error> {
        run_hooks_for_changeset(
            &self.ctx,
            &self.repo,
            self.hook_manager.as_ref(),
            &self.bookmark,
            changeset,
        )
        .await
    }

    pub fn run_with_limit<'a>(
        &'a self,
        limit: usize,
    ) -> impl Stream<Item = Result<HookExecutionInstance, Error>> + 'a {
        async move {
            let bm_rev = self
                .repo
                .get_bonsai_bookmark(self.ctx.clone(), &self.bookmark)
                .compat()
                .await?
                .ok_or_else(|| ErrorKind::NoSuchBookmark(self.bookmark.clone()))?;

            let stream = AncestorsNodeStream::new(
                self.ctx.clone(),
                &self.repo.get_changeset_fetcher(),
                bm_rev,
            )
            .compat()
            .take(limit)
            .try_filter(move |cs_id| future::ready(!self.excludes.contains(cs_id)))
            .map(move |cs_id| async move {
                match cs_id {
                    Ok(cs_id) => {
                        cloned!(self.ctx, self.repo, self.hook_manager, self.bookmark);

                        let outcomes = task::spawn(async move {
                            run_hooks_for_changeset(
                                &ctx,
                                &repo,
                                hook_manager.as_ref(),
                                &bookmark,
                                cs_id,
                            )
                            .await
                        })
                        .await??;

                        Ok(outcomes)
                    }
                    Err(e) => Err(e),
                }
            })
            .buffered(100);

            Ok(stream)
        }
        .try_flatten_stream()
    }
}

async fn run_hooks_for_changeset(
    ctx: &CoreContext,
    repo: &BlobRepo,
    hm: &HookManager,
    bm: &BookmarkName,
    cs_id: ChangesetId,
) -> Result<HookExecutionInstance, Error> {
    let cs = cs_id.load(ctx.clone(), repo.blobstore()).compat().await?;

    debug!(ctx.logger(), "Running hooks for changeset {:?}", cs);

    let file_count = cs.file_changes_map().len();

    let (stats, outcomes) = hm
        .run_hooks_for_bookmark(ctx, vec![cs].iter(), bm, None)
        .timed()
        .await;

    let outcomes = outcomes?;

    Ok(HookExecutionInstance {
        cs_id,
        file_count,
        stats,
        outcomes,
    })
}

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("No such bookmark '{0}'")]
    NoSuchBookmark(BookmarkName),
}
