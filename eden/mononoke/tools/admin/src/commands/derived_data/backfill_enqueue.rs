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
use mononoke_app::MononokeApp;
use mononoke_app::args::ChangesetArgs;
use mononoke_app::args::DerivedDataArgs;
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

    /// Whether to rederive already-derived changesets
    #[clap(long)]
    pub(crate) rederive: bool,

    /// Whether to compute slices as if all commits were underived
    #[clap(long)]
    reslice: bool,

    /// Repositories to backfill (comma-separated names).
    /// If provided, overrides the top-level --repo-name/--repo-id.
    #[clap(long, value_delimiter = ',')]
    repos: Vec<String>,
}

pub(super) async fn backfill_enqueue(
    ctx: &CoreContext,
    app: &MononokeApp,
    default_repo: &Repo,
    queue: AsyncMethodRequestQueue,
    args: BackfillEnqueueArgs,
    config_name: Option<&str>,
    bypass_redaction: bool,
) -> Result<()> {
    let derived_data_type = args.derived_data_args.resolve_type()?;

    // Collect all repos to process
    let repos: Vec<Repo> = if args.repos.is_empty() {
        vec![]
    } else {
        let mut opened = Vec::new();
        for repo_name in &args.repos {
            let repo_arg = RepoArgs::from_repo_name(repo_name.clone());
            let repo: Repo =
                super::open_repo_for_derive(app, &repo_arg, args.rederive, bypass_redaction)
                    .await
                    .with_context(|| format!("Failed to open repo: {}", repo_name))?;
            opened.push(repo);
        }
        opened
    };

    // Build repo entries: resolve changesets for each repo
    let mut repo_entries: Vec<thrift::DeriveBackfillRepoEntry> = Vec::new();

    if repos.is_empty() {
        // Single repo mode: use the default repo
        let cs_ids = args
            .changeset_args
            .resolve_changesets(ctx, default_repo)
            .await?;
        let repo_id = default_repo.repo_identity().id();
        let cs_id_bytes: Vec<Vec<u8>> = cs_ids.iter().map(|cs| cs.as_ref().to_vec()).collect();
        info!(
            "Resolved {} changesets for repo {} ({})",
            cs_ids.len(),
            repo_id,
            default_repo.repo_identity().name(),
        );
        repo_entries.push(thrift::DeriveBackfillRepoEntry {
            repo_id: repo_id.id() as i64,
            cs_ids: cs_id_bytes,
            ..Default::default()
        });
    } else {
        for repo in &repos {
            let cs_ids = args.changeset_args.resolve_changesets(ctx, repo).await?;
            let repo_id = repo.repo_identity().id();
            let cs_id_bytes: Vec<Vec<u8>> = cs_ids.iter().map(|cs| cs.as_ref().to_vec()).collect();
            info!(
                "Resolved {} changesets for repo {} ({})",
                cs_ids.len(),
                repo_id,
                repo.repo_identity().name(),
            );
            repo_entries.push(thrift::DeriveBackfillRepoEntry {
                repo_id: repo_id.id() as i64,
                cs_ids: cs_id_bytes,
                ..Default::default()
            });
        }
    }

    let params = thrift::DeriveBackfillParams {
        derived_data_type: derived_data_type.name().to_string(),
        repo_entries: repo_entries.clone(),
        slice_size: args.slice_size as i64,
        boundaries_concurrency: args.boundaries_concurrency,
        num_boundary_requests: args.num_boundary_requests as i32,
        rederive: args.rederive,
        reslice: args.reslice,
        config_name: config_name.map(|s| s.to_string()),
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
