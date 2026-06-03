/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use anyhow::anyhow;
use clap::Args;
use context::CoreContext;
use repo_derivation_queues::RepoDerivationQueuesRef;
use repo_identity::RepoIdentityRef;
use tracing::info;

use super::Repo;

/// Delete every item in the derivation queue for this `(repo, config)`.
/// Iterates all DAG node types (needed, ready, ready-lowpri, deriving,
/// deps, rdeps) and removes their znodes via batched Zeus MultiOps.
/// Aborts on the first Zeus error.
///
/// Intended for unwedging a stuck queue. Pause the derivation service
/// before running.
#[derive(Args)]
pub struct NukeArgs {
    /// Required acknowledgement that this command will permanently delete
    /// all items in the queue for the selected (repo, config).
    #[clap(long)]
    i_understand_this_is_destructive: bool,
}

pub async fn nuke(ctx: &CoreContext, repo: &Repo, config_name: &str, args: NukeArgs) -> Result<()> {
    if !args.i_understand_this_is_destructive {
        return Err(anyhow!(
            "Refusing to nuke without --i-understand-this-is-destructive",
        ));
    }

    let derivation_queue = repo
        .repo_derivation_queues()
        .queue(config_name)
        .ok_or_else(|| anyhow!("Missing derivation queue for config {config_name}"))?;

    let repo_name = repo.repo_identity().name();
    info!(
        "Nuking derivation queue for repo={} config={}",
        repo_name, config_name,
    );

    let stats = derivation_queue.unsafe_nuke(ctx).await?;

    let total: u64 = stats.deleted_per_type.values().sum();
    info!(
        "Nuke complete for repo={} config={}: {} items deleted total",
        repo_name, config_name, total,
    );
    for (node_type, count) in &stats.deleted_per_type {
        info!("  {}: {}", node_type, count);
    }

    Ok(())
}
