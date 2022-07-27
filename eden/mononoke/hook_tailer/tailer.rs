/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use anyhow::Result;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use bookmarks::BookmarkName;
use cloned::cloned;
use context::CoreContext;
use futures::compat::Stream01CompatExt;
use futures::future;
use futures::future::TryFutureExt;
use futures::stream;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures_stats::FutureStats;
use futures_stats::TimedFutureExt;
use hooks::hook_loader::load_hooks;
use hooks::CrossRepoPushSource;
use hooks::HookManager;
use hooks::HookOutcome;
use hooks::PushAuthoredBy;
use hooks_content_stores::repo_text_only_fetcher;
use metaconfig_types::RepoConfig;
use mononoke_types::ChangesetId;
use permission_checker::AclProvider;
use revset::AncestorsNodeStream;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::debug;
use slog::info;
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
    concurrency: usize,
    log_interval: usize,
    exclude_merges: bool,
    excludes: HashSet<ChangesetId>,
    cross_repo_push_source: CrossRepoPushSource,
    push_authored_by: PushAuthoredBy,
}

impl Tailer {
    pub async fn new(
        ctx: CoreContext,
        acl_provider: &dyn AclProvider,
        repo: BlobRepo,
        config: RepoConfig,
        bookmark: BookmarkName,
        concurrency: usize,
        log_interval: usize,
        exclude_merges: bool,
        excludes: HashSet<ChangesetId>,
        disabled_hooks: &HashSet<String>,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
    ) -> Result<Tailer> {
        let content_fetcher = repo_text_only_fetcher(&repo, config.hook_max_file_size);

        let mut hook_manager = HookManager::new(
            ctx.fb,
            acl_provider,
            content_fetcher,
            config.hook_manager_params.clone().unwrap_or_default(),
            MononokeScubaSampleBuilder::with_discard(),
            repo.name().clone(),
        )
        .await?;

        load_hooks(
            ctx.fb,
            acl_provider,
            &mut hook_manager,
            &config,
            disabled_hooks,
        )
        .await?;

        Ok(Tailer {
            ctx,
            repo,
            hook_manager: Arc::new(hook_manager),
            bookmark,
            concurrency,
            log_interval,
            exclude_merges,
            excludes,
            cross_repo_push_source,
            push_authored_by,
        })
    }

    pub fn run_changesets<'a, I>(
        &'a self,
        changesets: I,
    ) -> impl Stream<Item = Result<HookExecutionInstance, Error>> + 'a
    where
        I: IntoIterator<Item = ChangesetId> + 'a,
    {
        let stream = stream::iter(changesets.into_iter().map(Ok));
        self.run_on_stream(stream)
    }

    pub fn run_with_limit<'a>(
        &'a self,
        limit: usize,
    ) -> impl Stream<Item = Result<HookExecutionInstance, Error>> + 'a {
        async move {
            let bm_rev = self
                .repo
                .get_bonsai_bookmark(self.ctx.clone(), &self.bookmark)
                .await?
                .ok_or_else(|| ErrorKind::NoSuchBookmark(self.bookmark.clone()))?;

            let stream = AncestorsNodeStream::new(
                self.ctx.clone(),
                &self.repo.get_changeset_fetcher(),
                bm_rev,
            )
            .compat()
            .take(limit);

            Ok(self.run_on_stream(stream))
        }
        .try_flatten_stream()
    }

    fn run_on_stream<'a, S>(
        &'a self,
        stream: S,
    ) -> impl Stream<Item = Result<HookExecutionInstance, Error>> + 'a
    where
        S: Stream<Item = Result<ChangesetId, Error>> + 'a,
    {
        let mut count = 0;
        stream
            .try_filter(move |cs_id| future::ready(!self.excludes.contains(cs_id)))
            .inspect_ok(move |cs_id| {
                if count % self.log_interval == 0 {
                    info!(
                        self.ctx.logger(),
                        "Starting hooks for {} ({} already started)", cs_id, count
                    );
                }
                count += 1;
            })
            .map(move |cs_id| async move {
                match cs_id {
                    Ok(cs_id) => {
                        cloned!(
                            self.ctx,
                            self.repo,
                            self.hook_manager,
                            self.bookmark,
                            self.exclude_merges,
                            self.cross_repo_push_source,
                            self.push_authored_by,
                        );

                        let maybe_outcomes = task::spawn(async move {
                            run_hooks_for_changeset(
                                &ctx,
                                &repo,
                                hook_manager.as_ref(),
                                &bookmark,
                                cs_id,
                                exclude_merges,
                                cross_repo_push_source,
                                push_authored_by,
                            )
                            .await
                        })
                        .await??;

                        Ok(maybe_outcomes)
                    }
                    Err(e) => Err(e),
                }
            })
            .buffered(self.concurrency)
            .try_filter_map(|maybe_outcomes| future::ready(Ok(maybe_outcomes)))
    }
}

async fn run_hooks_for_changeset(
    ctx: &CoreContext,
    repo: &BlobRepo,
    hm: &HookManager,
    bm: &BookmarkName,
    cs_id: ChangesetId,
    exclude_merges: bool,
    cross_repo_push_source: CrossRepoPushSource,
    push_authored_by: PushAuthoredBy,
) -> Result<Option<HookExecutionInstance>, Error> {
    let cs = cs_id.load(ctx, repo.blobstore()).await?;

    if exclude_merges && cs.is_merge() {
        info!(ctx.logger(), "Skipped merge commit {}", cs_id);
        return Ok(None);
    }

    debug!(ctx.logger(), "Running hooks for changeset {:?}", cs);

    let file_count = cs.file_changes_map().len();

    let (stats, outcomes) = hm
        .run_hooks_for_bookmark(
            ctx,
            vec![cs].iter(),
            bm,
            None,
            cross_repo_push_source,
            push_authored_by,
        )
        .timed()
        .await;

    let outcomes = outcomes?;

    Ok(Some(HookExecutionInstance {
        cs_id,
        file_count,
        stats,
        outcomes,
    }))
}

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("No such bookmark '{0}'")]
    NoSuchBookmark(BookmarkName),
}
