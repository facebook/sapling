/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use anyhow::anyhow;
use bulk_derivation::BulkDerivation;
use clap::Args;
use clap::ValueEnum;
use context::CoreContext;
use mononoke_app::args::ChangesetArgs;
use mononoke_app::args::DerivedDataArgs;
use repo_derivation_queues::DerivationPriority;
use repo_derivation_queues::RepoDerivationQueuesRef;
use repo_derivation_queues::build_underived_batched_graph;
use repo_derivation_queues::derivation_priority_to_str;
use tracing::info;

use super::Repo;

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Priority {
    Low,
    High,
}

impl From<Priority> for DerivationPriority {
    fn from(p: Priority) -> Self {
        match p {
            Priority::Low => DerivationPriority::LOW,
            Priority::High => DerivationPriority::HIGH,
        }
    }
}

#[derive(Args)]
pub struct EnqueueArgs {
    #[clap(flatten)]
    changeset_args: ChangesetArgs,

    #[clap(flatten)]
    derived_data_args: DerivedDataArgs,

    /// Priority for the derivation request
    #[clap(long, short, default_value = "low")]
    priority: Priority,

    /// Batch size for derivation (number of commits per batch)
    #[clap(long, default_value = "100")]
    batch_size: u64,

    /// Whether to wait for derivation to complete
    #[clap(long)]
    wait: bool,
}

pub async fn enqueue(
    ctx: &CoreContext,
    repo: &Repo,
    config_name: &str,
    args: EnqueueArgs,
) -> Result<()> {
    let derivation_queue = repo
        .repo_derivation_queues()
        .queue(config_name)
        .ok_or_else(|| anyhow!("Missing derivation queue for config {}", config_name))?;

    let ddm = derivation_queue.derived_data_manager();
    let derived_data_type = args.derived_data_args.resolve_type()?;
    let cs_id = args.changeset_args.resolve_changeset(ctx, repo).await?;

    // Check if already derived
    if ddm.is_derived(ctx, cs_id, None, derived_data_type).await? {
        info!(
            "Changeset {} is already derived for type {}",
            cs_id, derived_data_type
        );
        return Ok(());
    }

    let priority: DerivationPriority = args.priority.into();
    let batch_size = args.batch_size;

    info!(
        "Enqueuing derivation for cs_id={}, derived_data_type={}, priority={}, batch_size={}",
        cs_id,
        derived_data_type,
        derivation_priority_to_str(priority),
        batch_size
    );

    let response = build_underived_batched_graph(
        ctx,
        Arc::clone(&derivation_queue),
        ddm,
        derived_data_type,
        cs_id,
        None, // bubble_id - not supporting ephemeral derivation for now
        batch_size,
        Some(priority),
    )
    .await?;

    match response {
        Some(watch) => {
            info!(
                "Enqueued derivation for cs_id={}, derived_data_type={}",
                cs_id, derived_data_type
            );
            if args.wait {
                info!("Waiting for derivation to complete...");
                let is_derived = watch.is_derived().await?;
                if is_derived {
                    info!("Derivation completed successfully");
                } else {
                    // This happens if the queue watch fired with a non-delete event,
                    // meaning the item is still in the queue. Use `derivation-queue summary`
                    // to check status, or re-run with --wait to continue waiting.
                    info!(
                        "Watch fired but derivation not yet complete - item may still be in queue"
                    );
                }
            }
        }
        None => {
            info!("No items to enqueue (changeset may already be derived or queued)");
        }
    }

    Ok(())
}
