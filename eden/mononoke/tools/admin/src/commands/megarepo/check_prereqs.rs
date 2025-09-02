/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use bookmarks::BookmarkKey;
use bookmarks::BookmarksRef;
use clap::Args;
use cmdlib_cross_repo::repo_provider_from_mononoke_app;
use commit_id::parse_commit_id;
use context::CoreContext;
use cross_repo_sync::CommitSyncData;
use cross_repo_sync::CommitSyncRepos;
use cross_repo_sync::Source;
use cross_repo_sync::Target;
use cross_repo_sync::get_all_submodule_deps_from_repo_pair;
use cross_repo_sync::verify_working_copy_with_version;
use futures::future::try_join;
use metaconfig_types::CommitSyncConfigVersion;
use mononoke_api::Repo;
use mononoke_app::MononokeApp;
use mononoke_app::args::SourceRepoArgs;
use mononoke_app::args::TargetRepoArgs;
use repo_identity::RepoIdentityRef;
use slog::info;

use crate::commands::megarepo::common::get_live_commit_sync_config;

/// check the prerequisites of enabling push-redirection at a given commit with a given CommitSyncConfig version
#[derive(Args)]
pub struct CheckPrereqsArgs {
    /// changeset hashes or bookmarks to check. First one is the source, second one is the target
    #[arg(long)]
    source_changeset: String,

    #[arg(long)]
    target_changeset: String,

    #[arg(long)]
    version: String,

    #[clap(flatten)]
    source_repo: SourceRepoArgs,

    #[clap(flatten)]
    target_repo: TargetRepoArgs,
}

pub async fn run(ctx: &CoreContext, app: MononokeApp, args: CheckPrereqsArgs) -> Result<()> {
    let target_repo_fut = app.open_repo(&args.target_repo);
    let source_repo_fut = app.open_repo(&args.source_repo);

    let (source_repo, target_repo): (Repo, Repo) =
        try_join(source_repo_fut, target_repo_fut).await?;

    info!(
        ctx.logger(),
        "Resolving source changeset in {}",
        source_repo.repo_identity().name(),
    );

    let source_cs_id = if let Some(bookmark) = args.source_changeset.strip_prefix("bm=") {
        source_repo
            .bookmarks()
            .get(
                ctx.clone(),
                &BookmarkKey::new(bookmark)?,
                bookmarks::Freshness::MostRecent,
            )
            .await
            .with_context(|| format!("Failed to resolve bookmark '{}'", bookmark))?
            .ok_or_else(|| anyhow!("Couldn't find bookmark: {}", bookmark))
    } else {
        parse_commit_id(ctx, &source_repo, &args.source_changeset).await
    }?;

    info!(
        ctx.logger(),
        "Resolving target changeset in {}",
        target_repo.repo_identity().name()
    );

    let target_cs_id = if let Some(bookmark) = args.target_changeset.strip_prefix("bm=") {
        target_repo
            .bookmarks()
            .get(
                ctx.clone(),
                &BookmarkKey::new(bookmark)?,
                bookmarks::Freshness::MostRecent,
            )
            .await
            .with_context(|| format!("Failed to resolve bookmark '{}'", bookmark))?
            .ok_or_else(|| anyhow!("Couldn't find bookmark: {}", bookmark))
    } else {
        parse_commit_id(ctx, &target_repo, &args.target_changeset).await
    }?;

    let version = CommitSyncConfigVersion(args.version);

    info!(
        ctx.logger(),
        "Checking push-redirection prerequisites for {}({})->{}({}), {:?}",
        source_cs_id,
        source_repo.repo_identity().name(),
        target_cs_id,
        target_repo.repo_identity().name(),
        version,
    );

    let live_commit_sync_config = get_live_commit_sync_config(ctx, &app, &args.source_repo)
        .await
        .context("building live_commit_sync_config")?;

    let repo_provider = repo_provider_from_mononoke_app(&app);

    let source_repo_arc = Arc::new(source_repo.clone());
    let target_repo_arc = Arc::new(target_repo.clone());
    let submodule_deps = get_all_submodule_deps_from_repo_pair(
        ctx,
        source_repo_arc.clone(),
        target_repo_arc.clone(),
        repo_provider,
    )
    .await?;

    let commit_sync_repos =
        CommitSyncRepos::from_source_and_target_repos(source_repo, target_repo, submodule_deps)?;

    let commit_sync_data =
        CommitSyncData::new(ctx, commit_sync_repos, live_commit_sync_config.clone());

    verify_working_copy_with_version(
        ctx,
        &commit_sync_data,
        Source(source_cs_id),
        Target(target_cs_id),
        &version,
        live_commit_sync_config,
    )
    .await
}
