/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use async_requests::AsyncMethodRequestQueue;
use async_requests::types::Token;
use clap::Args;
use context::CoreContext;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use metaconfig_types::CommitIdentityScheme;
use mononoke_app::MononokeApp;
use mononoke_app::args::ChangesetArgs;
use mononoke_app::args::DerivedDataArgs;
use mononoke_app::args::RepoArg;
use mononoke_app::args::RepoArgs;
use repo_identity::RepoIdentityRef;
use source_control as thrift;
use tracing::info;

use super::Repo;

#[derive(Args)]
pub(super) struct BackfillEnqueueArgs {
    #[clap(flatten)]
    changeset_args: ChangesetArgs,

    #[clap(flatten)]
    derived_data_args: DerivedDataArgs,

    /// The size of each slice in generation numbers
    #[clap(long, default_value_t = 50000)]
    slice_size: u64,

    /// Concurrency for boundary derivation requests
    #[clap(long, default_value_t = 10)]
    boundaries_concurrency: i32,

    /// Number of separate boundary derivation requests to create for parallelization
    #[clap(long, default_value_t = 10)]
    num_boundary_requests: usize,

    /// Maximum number of repos to derive concurrently. The worker keeps at most
    /// this many repos in flight and schedules a new one as each repo finishes.
    /// 0 (the default) means no limit: schedule all repos at once.
    #[clap(long, default_value_t = 0)]
    repo_concurrency: usize,

    /// Whether to compute slices as if all commits were underived
    #[clap(long)]
    reslice: bool,

    /// Enqueue a backfill for every enabled git repo in the loaded config/tier
    /// (run with `--git-config` so the git tier's manifest is loaded). Mutually
    /// exclusive with `-R`/`--repo-id`; combine with `--repo-concurrency` to
    /// bound how many repos derive at once.
    #[clap(long)]
    all_git_repos: bool,

    /// Enqueue a MarkTypeEnabled node per repo that records the type as enabled
    /// in the enabled_derived_data_types table once the repo's backfill succeeds.
    #[clap(long)]
    auto_enable: bool,
}

pub(super) async fn backfill_enqueue(
    ctx: &CoreContext,
    app: &MononokeApp,
    queue: AsyncMethodRequestQueue,
    args: BackfillEnqueueArgs,
    repo_args: &[RepoArg],
    config_name: Option<&str>,
    bypass_redaction: bool,
) -> Result<()> {
    anyhow::ensure!(
        args.all_git_repos || !repo_args.is_empty(),
        "one of --repo-id / --repo-name or --all-git-repos is required"
    );
    anyhow::ensure!(
        !args.all_git_repos || repo_args.is_empty(),
        "--all-git-repos cannot be combined with --repo-id / --repo-name"
    );

    let derived_data_type = args.derived_data_args.resolve_type()?;

    // Resolve the set of repos to backfill. `--all-git-repos` enumerates every
    // enabled git repo in the loaded config/tier (so it must be run with
    // `--git-config`); otherwise use the explicitly-requested repos. Both
    // paths converge on the same per-repo open + resolve loop below.
    let repo_targets: Vec<RepoArgs> = if args.all_git_repos {
        let targets: Vec<RepoArgs> = app
            .configs()
            .load_all_repo_configs()
            .context("Failed to load repo configs for --all-git-repos")?
            .into_iter()
            .filter(|(_, config)| {
                config.enabled && config.default_commit_identity_scheme == CommitIdentityScheme::GIT
            })
            .map(|(name, _)| RepoArgs::from_repo_name(name))
            .collect();
        anyhow::ensure!(
            !targets.is_empty(),
            "--all-git-repos found no enabled git repos in the loaded config \
             (did you pass --git-config?)"
        );
        info!("--all-git-repos: enqueueing {} git repos", targets.len());
        targets
    } else {
        repo_args
            .iter()
            .map(|repo_arg| match repo_arg {
                RepoArg::Id(id) => RepoArgs::from_repo_id(id.id()),
                RepoArg::Name(name) => RepoArgs::from_repo_name(name.clone()),
            })
            .collect()
    };

    // Open each repo and resolve its changesets concurrently (bounded) rather than
    // one at a time -- with `--all-git-repos` this spans thousands of independent
    // repo opens + bookmark-head resolutions, so a sequential loop dominates
    // enqueue latency. Run with `--skip-preloading-commit-graph` so each open is a
    // light SQL handle (enqueue only needs bookmark heads, never the graph). Order
    // of `repo_entries` is not significant to the scheduler.
    const ENQUEUE_RESOLVE_CONCURRENCY: usize = 100;
    let changeset_args = &args.changeset_args;
    let repo_entries: Vec<thrift::DeriveBackfillRepoEntry> = stream::iter(&repo_targets)
        .map(|repo_arg| async move {
            let repo: Repo = super::open_repo_for_derive(app, repo_arg, false, bypass_redaction)
                .await
                .context("Failed to open repo")?;
            let cs_ids = changeset_args.resolve_changesets(ctx, &repo).await?;
            let repo_id = repo.repo_identity().id();
            let cs_id_bytes: Vec<Vec<u8>> = cs_ids.iter().map(|cs| cs.as_ref().to_vec()).collect();
            info!(
                "Resolved {} changesets for repo {} ({})",
                cs_ids.len(),
                repo_id,
                repo.repo_identity().name(),
            );
            anyhow::Ok(thrift::DeriveBackfillRepoEntry {
                repo_id: repo_id.id() as i64,
                cs_ids: cs_id_bytes,
                ..Default::default()
            })
        })
        .buffer_unordered(ENQUEUE_RESOLVE_CONCURRENCY)
        .try_collect()
        .await?;

    let params = thrift::DeriveBackfillParams {
        derived_data_type: derived_data_type.name().to_string(),
        repo_entries: repo_entries.clone(),
        slice_size: args.slice_size as i64,
        boundaries_concurrency: args.boundaries_concurrency,
        num_boundary_requests: args.num_boundary_requests as i32,
        reslice: args.reslice,
        auto_enable: Some(args.auto_enable),
        config_name: config_name.map(|s| s.to_string()),
        repo_concurrency: Some(args.repo_concurrency as i32),
        ..Default::default()
    };

    // Routing: 1 repo -> repo queue, N>1 -> global queue
    let queue_repo_id = if repo_entries.len() == 1 {
        let single_repo_id = mononoke_types::RepositoryId::new(repo_entries[0].repo_id as i32);
        Some(single_repo_id)
    } else {
        None
    };

    let token = queue
        .enqueue(ctx, queue_repo_id.as_ref(), params)
        .await
        .context("Failed to enqueue DeriveBackfill request")?;

    info!(
        "Enqueued DeriveBackfill request (id={}, {} repos)",
        token.id().0,
        repo_entries.len(),
    );

    Ok(())
}
