/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::Result;
use anyhow::anyhow;
use bulk_derivation::BulkDerivation;
use bulk_derivation::derive_stage_batch;
use clap::Args;
use context::CoreContext;
use context::SessionClass;
use derived_data_manager::DerivationStagePayload;
use derived_data_manager::DerivedDataManager;
use derived_data_manager::ManifestStagePayload;
use derived_data_manager::Rederivation;
use futures_stats::TimedTryFutureExt;
use itertools::Itertools;
use mononoke_api::ChangesetId;
use mononoke_app::args::ChangesetArgs;
use mononoke_app::args::MultiDerivedDataArgs;
use mononoke_types::DerivableType;
use mononoke_types::MPath;
use tracing::debug;

use super::Repo;

#[derive(Args)]
pub(super) struct DeriveArgs {
    #[clap(flatten)]
    changeset_args: ChangesetArgs,
    #[clap(flatten)]
    multi_derived_data_args: MultiDerivedDataArgs,
    /// Whether the changesets need to be rederived or not
    #[clap(long)]
    pub(crate) rederive: bool,
    /// Whether to derive the type without deriving its parents. Usable
    /// only with types that implement DerivableUntopologically.
    #[clap(long)]
    unsafe_derive_untopologically: bool,
    /// Batch size to use for derivation
    #[clap(long)]
    batch_size: Option<u64>,
    /// Derive only a single pipeline stage, identified by its absolute path
    /// (e.g. `""` for the root, or `"fbcode"`). Only supported for types
    /// with pipeline derivation (fsnodes, unodes).
    #[clap(long, value_parser = parse_mpath)]
    stage_path: Option<MPath>,
}

fn parse_mpath(s: &str) -> Result<MPath> {
    MPath::new(s.as_bytes())
}

pub(super) async fn derive(
    ctx: &mut CoreContext,
    repo: &Repo,
    manager: &DerivedDataManager,
    args: DeriveArgs,
) -> Result<()> {
    let resolved_csids = args.changeset_args.resolve_changesets(ctx, repo).await?;
    let csids = resolved_csids.as_slice();

    let derived_data_types = args
        .multi_derived_data_args
        .resolve_types(manager.config())?;

    let rederivation = if args.rederive {
        debug!("about to rederive {} commits", csids.len());
        // Force this binary to write to all blobstores
        ctx.session_mut()
            .override_session_class(SessionClass::Background);
        Arc::new(Mutex::new(
            derived_data_types
                .iter()
                .copied()
                .cartesian_product(csids.iter().copied())
                .collect::<HashSet<_>>(),
        ))
    } else {
        debug!("about to derive {} commits", csids.len());
        Default::default()
    };

    if let Some(stage_path) = args.stage_path {
        let pipeline_config = manager
            .repo_config()
            .derived_data_config
            .pipeline_config
            .as_ref()
            .ok_or_else(|| anyhow!("repo has no derivation pipeline config"))?;
        let stage = pipeline_config
            .stages
            .get(&stage_path)
            .ok_or_else(|| anyhow!("Pipeline config has no stage at path {}", stage_path))?;
        let deps = stage
            .dependencies
            .iter()
            .map(|dep_path| {
                dep_path
                    .iter()
                    .last()
                    .expect("validator: dep path is non-empty")
                    .clone()
            })
            .collect();
        let payload = DerivationStagePayload::Manifest(ManifestStagePayload {
            path: stage_path.clone(),
            deps,
        });
        for derived_data_type in &derived_data_types {
            let variant = derived_data_type.into_pipeline_derivable_variant()?;

            debug!(
                "about to derive stage at {} of {} for {} commits",
                stage_path,
                derived_data_type.name(),
                csids.len()
            );

            let duration =
                derive_stage_batch(manager, ctx, csids.to_vec(), &payload, variant).await?;

            debug!(
                "Stage at {} derivation for {} completed in {}ms",
                stage_path,
                derived_data_type.name(),
                duration.as_millis()
            );
        }
    } else if args.unsafe_derive_untopologically {
        for derived_data_type in derived_data_types {
            for csid in csids {
                unsafe_derive_untopologically(
                    ctx,
                    manager,
                    derived_data_type,
                    *csid,
                    rederivation.clone(),
                )
                .await?;
            }
        }
    } else {
        let (stats, ()) = manager
            .derive_bulk_locally(
                ctx,
                csids,
                Some(rederivation),
                &derived_data_types,
                args.batch_size,
            )
            .try_timed()
            .await?;
        debug!(
            "finished derivation in {}ms",
            stats.completion_time.as_millis(),
        );
    }

    Ok(())
}

async fn unsafe_derive_untopologically(
    ctx: &CoreContext,
    manager: &DerivedDataManager,
    derived_data_type: DerivableType,
    csid: ChangesetId,
    rederivation: Arc<dyn Rederivation>,
) -> Result<()> {
    debug!("deriving {} from predecessors", csid);
    let (stats, res) = BulkDerivation::unsafe_derive_untopologically(
        manager,
        ctx,
        csid,
        Some(rederivation),
        derived_data_type,
    )
    .try_timed()
    .await?;
    debug!(
        "derived {} for {} in {}ms, {:?}",
        derived_data_type.name(),
        csid,
        stats.completion_time.as_millis(),
        res,
    );
    Ok(())
}
