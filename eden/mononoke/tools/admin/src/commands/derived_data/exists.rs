/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use bulk_derivation::BulkDerivation;
use clap::builder::PossibleValuesParser;
use clap::Args;
use context::CoreContext;
use mononoke_app::args::ChangesetArgs;
use mononoke_types::DerivableType;
use repo_derived_data::RepoDerivedDataRef;
use strum::IntoEnumIterator;

use super::Repo;

#[derive(Args)]
pub(super) struct ExistsArgs {
    #[clap(flatten)]
    changeset_args: ChangesetArgs,

    /// The derived data config name to use. If not specified, the enabled config will be used
    #[clap(short, long)]
    config_name: Option<String>,

    #[clap(short = 'T', long, value_parser = PossibleValuesParser::new(DerivableType::iter().map(|t| DerivableType::name(&t))))]
    derived_data_type: String,
}

pub(super) async fn exists(ctx: &CoreContext, repo: &Repo, args: ExistsArgs) -> Result<()> {
    let cs_ids = args.changeset_args.resolve_changesets(ctx, repo).await?;
    let derived_data_type = DerivableType::from_name(&args.derived_data_type)?;

    let manager = if let Some(config_name) = args.config_name {
        repo.repo_derived_data().manager_for_config(&config_name)?
    } else {
        repo.repo_derived_data().manager()
    };

    let pending = manager
        .pending(ctx, &cs_ids, None, derived_data_type)
        .await?;

    for cs_id in cs_ids {
        if pending.contains(&cs_id) {
            println!("Not Derived: {}", cs_id);
        } else {
            println!("Derived: {}", cs_id);
        }
    }

    Ok(())
}
