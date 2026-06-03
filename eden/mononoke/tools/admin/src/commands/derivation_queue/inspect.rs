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
use mononoke_app::args::ChangesetArgs;
use mononoke_app::args::DerivedDataArgs;
use mononoke_types::MPath;
use repo_derivation_queues::DagItemId;
use repo_derivation_queues::ReadyState;
use repo_derivation_queues::RepoDerivationQueuesRef;
use repo_derivation_queues::derivation_priority_to_str;
use repo_identity::RepoIdentityRef;

use super::Repo;

#[derive(Args)]
pub struct InspectArgs {
    #[clap(flatten)]
    changeset_args: ChangesetArgs,

    #[clap(flatten)]
    derived_data_args: DerivedDataArgs,

    /// Absolute path of the pipeline stage (e.g. `""` for root, `"fbcode"`).
    /// Omit for non-pipeline items.
    #[clap(long, value_parser = |s: &str| MPath::new(s.as_bytes()))]
    stage_path: Option<MPath>,
}

pub async fn inspect(
    ctx: &CoreContext,
    repo: &Repo,
    config_name: &str,
    args: InspectArgs,
) -> Result<()> {
    let derivation_queue = repo
        .repo_derivation_queues()
        .queue(config_name)
        .ok_or_else(|| anyhow!("Missing derivation queue for config {config_name}"))?;

    let derived_data_type = args.derived_data_args.resolve_type()?;
    let cs_ids = args.changeset_args.resolve_changesets(ctx, repo).await?;
    let stage_hash = args.stage_path.as_ref().map(|p| p.get_path_hash());

    for cs_id in cs_ids {
        let item_id = DagItemId::new(
            repo.repo_identity().id(),
            config_name.to_string(),
            derived_data_type,
            cs_id,
            stage_hash,
        );

        println!(
            "Item: {:?}/{}/{}",
            derived_data_type,
            args.stage_path
                .as_ref()
                .map(|p| p.to_string())
                .unwrap_or_else(|| "(no stage)".to_string()),
            cs_id
        );

        let result = derivation_queue.inspect(ctx, item_id).await?;

        // needed / ready / deriving / info are printed as independent
        // fields so inconsistent states (e.g. info present in ready but
        // needed znode missing) are visible rather than hidden.
        println!(
            "  needed:   {}",
            if result.needed_exists {
                "EXISTS"
            } else {
                "MISSING"
            }
        );
        let ready_str = match result.ready_state {
            ReadyState::NotReady => "no",
            ReadyState::ReadyHighPri => "YES (high priority)",
            ReadyState::ReadyLowPri => "YES (low priority)",
        };
        println!("  ready:    {ready_str}");
        println!(
            "  deriving: {}",
            if result.is_deriving { "YES" } else { "no" }
        );
        match &result.info {
            Some(info) => {
                let ts = info
                    .enqueue_timestamp()
                    .map(|t| format!("{}s{}ms ago", t.since_seconds(), t.since_millis() % 1000))
                    .unwrap_or_else(|| "unknown".to_string());
                println!(
                    "  info:     retry_count={}, priority={}, enqueued {}",
                    info.retry_count(),
                    derivation_priority_to_str(info.priority()),
                    ts
                );
                println!("  head:     {}", info.head_cs_id());
            }
            None => println!("  info:     (none)"),
        }

        if result.forward_deps.is_empty() {
            println!("  forward deps: (none)");
        } else {
            println!("  forward deps ({}):", result.forward_deps.len());
            for dep in &result.forward_deps {
                let status = if dep.needed_exists {
                    "EXISTS"
                } else {
                    "MISSING"
                };
                let marker = if !dep.needed_exists { " <- BROKEN" } else { "" };
                println!("    {} -> needed: {}{}", dep.suffix, status, marker);
            }
        }

        if result.reverse_deps.is_empty() {
            println!("  reverse deps: (none)");
        } else {
            println!("  reverse deps ({}):", result.reverse_deps.len());
            for dep in &result.reverse_deps {
                let status = if dep.needed_exists {
                    "EXISTS"
                } else {
                    "MISSING"
                };
                let marker = if !dep.needed_exists { " <- BROKEN" } else { "" };
                println!("    {} -> needed: {}{}", dep.suffix, status, marker);
            }
        }

        println!();
    }

    Ok(())
}
