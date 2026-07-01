/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use anyhow::anyhow;
use bulk_derivation::BulkDerivation;
use clap::Args;
use context::CoreContext;
use derived_data_manager::DerivedDataManager;
use derived_data_manager::StageId;
use mononoke_app::args::ChangesetArgs;
use mononoke_app::args::DerivedDataArgs;
use mononoke_types::MPath;

use super::Repo;

#[derive(Args)]
pub(super) struct VerifyStageOutputArgs {
    #[clap(flatten)]
    changeset_args: ChangesetArgs,

    #[clap(flatten)]
    derived_data_args: DerivedDataArgs,

    /// Absolute path of the pipeline manifest stage to verify (e.g. `""` for
    /// root, `"fbcode"` for the fbcode subtree). Mutually exclusive with
    /// `--finalize`.
    #[clap(long, value_parser = |s: &str| MPath::new(s.as_bytes()), conflicts_with = "finalize")]
    stage_path: Option<MPath>,

    /// Verify the finalize stage instead of a manifest stage.
    #[clap(long)]
    finalize: bool,
}

pub(super) async fn verify_stage_output(
    ctx: &CoreContext,
    repo: &Repo,
    manager: &DerivedDataManager,
    args: VerifyStageOutputArgs,
) -> Result<()> {
    let cs_ids = args.changeset_args.resolve_changesets(ctx, repo).await?;
    let derived_data_type = args.derived_data_args.resolve_type()?;

    let pipeline_config = manager
        .repo_config()
        .derived_data_config
        .pipeline_config
        .as_ref()
        .ok_or_else(|| anyhow!("repo has no derivation pipeline config"))?;

    let stage = if args.finalize {
        StageId::Finalize
    } else {
        let stage_path = args
            .stage_path
            .ok_or_else(|| anyhow!("either --stage-path or --finalize is required"))?;
        if !pipeline_config.stages.contains_key(&stage_path) {
            return Err(anyhow!("Pipeline config has no stage at path {stage_path}"));
        }
        StageId::Manifest(stage_path)
    };

    for cs_id in cs_ids {
        match BulkDerivation::verify_stage_output(manager, ctx, cs_id, derived_data_type, &stage)
            .await
        {
            Ok(true) => println!("{cs_id}: Match"),
            Ok(false) => println!("{cs_id}: Mismatch"),
            Err(e) => println!("{cs_id}: Error: {e:#}"),
        }
    }

    Ok(())
}
