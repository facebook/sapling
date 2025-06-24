/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use anyhow::Result;
use anyhow::anyhow;
use clap::Args;
use colored::Colorize;
use context::CoreContext;
use mononoke_app::args::ChangesetArgs;
use mononoke_app::args::DerivedDataArgs;
use repo_derivation_queues::DagItemId;
use repo_derivation_queues::RepoDerivationQueuesRef;
use repo_identity::RepoIdentityRef;
use slog::info;

use super::Repo;

#[derive(Args)]
pub struct UnsafeEvictArgs {
    #[clap(flatten)]
    changeset_args: ChangesetArgs,

    #[clap(flatten)]
    derived_data_args: DerivedDataArgs,
}

pub async fn unsafe_evict(
    ctx: &CoreContext,
    repo: &Repo,
    config_name: &str,
    args: UnsafeEvictArgs,
) -> Result<()> {
    print!(
        "{}",
        "Evicting an item from the derivation queue is unsafe and can lead to stuck derivations. Are you sure you want to continue? (y/n) "
            .red(),
    );
    std::io::stdout().flush()?;
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    match input.trim().to_lowercase().as_str() {
        "y" | "yes" => {}
        _ => {
            info!(ctx.logger(), "Eviction canceled");
            return Ok(());
        }
    }

    let derivation_queue = repo
        .repo_derivation_queues()
        .queue(config_name)
        .ok_or_else(|| anyhow!("Missing derivation queue for config {}", config_name))?;

    let derived_data_type = args.derived_data_args.resolve_type()?;
    let cs_id = args.changeset_args.resolve_changeset(ctx, repo).await?;

    derivation_queue
        .unsafe_evict(
            ctx,
            DagItemId::new(
                repo.repo_identity().id(),
                config_name.to_string(),
                derived_data_type,
                cs_id,
            ),
        )
        .await?;

    info!(
        ctx.logger(),
        "Evicted item cs_id={}, derived_data_type={}", cs_id, derived_data_type
    );

    Ok(())
}
