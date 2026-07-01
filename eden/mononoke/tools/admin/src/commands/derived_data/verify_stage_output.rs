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

    /// Absolute path of the pipeline stage to verify (e.g. `""` for root,
    /// `"fbcode"` for the fbcode subtree).
    #[clap(long, value_parser = |s: &str| MPath::new(s.as_bytes()))]
    stage_path: MPath,
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
    if !pipeline_config.stages.contains_key(&args.stage_path) {
        return Err(anyhow!(
            "Pipeline config has no stage at path {}",
            args.stage_path,
        ));
    }

    for cs_id in cs_ids {
        match BulkDerivation::verify_stage_output(
            manager,
            ctx,
            cs_id,
            derived_data_type,
            &StageId::Manifest(args.stage_path.clone()),
        )
        .await
        {
            Ok(true) => println!("{cs_id}: Match"),
            Ok(false) => println!("{cs_id}: Mismatch"),
            Err(e) => println!("{cs_id}: Error: {e:#}"),
        }
    }

    Ok(())
}
