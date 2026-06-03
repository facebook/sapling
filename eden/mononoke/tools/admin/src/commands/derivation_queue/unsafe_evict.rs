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
use futures::StreamExt;
use futures::stream;
use mononoke_app::args::ChangesetArgs;
use mononoke_app::args::DerivedDataArgs;
use mononoke_types::MPath;
use repo_derivation_queues::DagItemId;
use repo_derivation_queues::RepoDerivationQueuesRef;
use repo_identity::RepoIdentityRef;
use tracing::info;

use super::Repo;

#[derive(Args)]
pub struct UnsafeEvictArgs {
    #[clap(flatten)]
    changeset_args: ChangesetArgs,

    #[clap(flatten)]
    derived_data_args: DerivedDataArgs,

    /// Absolute path of the pipeline stage (e.g. `""` for root, `"fbcode"`).
    /// Omit for non-pipeline items.
    #[clap(long, value_parser = |s: &str| MPath::new(s.as_bytes()))]
    stage_path: Option<MPath>,

    /// Number of concurrent evictions
    #[clap(long, default_value_t = 100)]
    concurrency: usize,
}

pub async fn unsafe_evict(
    ctx: &CoreContext,
    repo: &Repo,
    config_name: &str,
    args: UnsafeEvictArgs,
) -> Result<()> {
    let derivation_queue = repo
        .repo_derivation_queues()
        .queue(config_name)
        .ok_or_else(|| anyhow!("Missing derivation queue for config {config_name}"))?;

    let derived_data_type = args.derived_data_args.resolve_type()?;
    let cs_ids = args.changeset_args.resolve_changesets(ctx, repo).await?;
    let stage_hash = args.stage_path.as_ref().map(|p| p.get_path_hash());

    info!(
        "Evicting {} items with concurrency {}",
        cs_ids.len(),
        args.concurrency
    );

    let results: Vec<_> = stream::iter(cs_ids)
        .map(async |cs_id| {
            let item_id = DagItemId::new(
                repo.repo_identity().id(),
                config_name.to_string(),
                derived_data_type,
                cs_id,
                stage_hash,
            );
            derivation_queue
                .unsafe_evict(ctx, item_id)
                .await
                .map_err(|e| (cs_id.to_string(), e))
        })
        .buffer_unordered(args.concurrency)
        .collect()
        .await;

    let success = results.iter().filter(|r| r.is_ok()).count();
    let failures: Vec<_> = results.into_iter().filter_map(|r| r.err()).collect();

    for (cs_id, err) in &failures {
        eprintln!("FAILED cs={cs_id}: {err:#}");
    }

    info!(
        "Evicted {}/{} items ({} failed), derived_data_type={}",
        success,
        success + failures.len(),
        failures.len(),
        derived_data_type
    );

    Ok(())
}
