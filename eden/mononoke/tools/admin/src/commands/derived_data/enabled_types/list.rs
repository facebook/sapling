/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use clap::Args;
use context::CoreContext;
use enabled_derived_data_types::EnabledDerivedDataTypesRef;
use mononoke_app::MononokeApp;
use mononoke_app::args::AsRepoArg;
use mononoke_app::args::OptRepoArgs;
use mononoke_types::DerivableType;
use prettytable::Table;
use prettytable::row;

use super::super::Repo;

#[derive(Args)]
pub(super) struct ListArgs {
    /// Optional repo, used only to open the (global) enabled-types facet. If
    /// omitted, the first configured repo is opened, since the table is global.
    #[clap(flatten)]
    repo: OptRepoArgs,

    /// Only show rows for this derived data type.
    #[clap(short = 'T', long)]
    r#type: Option<String>,
}

pub(super) async fn list(ctx: &CoreContext, app: &MononokeApp, args: ListArgs) -> Result<()> {
    // The enabled_derived_data_types table is global; we only need any repo
    // handle to reach the facet. Open the one named by -R if given, else the
    // first configured repo.
    let repo: Repo = match args.repo.as_repo_arg() {
        Some(repo_arg) => app.open_repo(repo_arg).await?,
        None => {
            let (_name, config) = app
                .configs()
                .load_all_repo_configs()?
                .into_iter()
                .min_by_key(|(_name, config)| config.repoid)
                .context("no repos are configured")?;
            app.open_named_repo(config.repoid).await?
        }
    };

    let type_filter = args
        .r#type
        .map(|t| DerivableType::from_name(&t))
        .transpose()?;

    let mut entries = repo.enabled_derived_data_types().get_all(ctx).await?;
    if let Some(ddt) = type_filter {
        entries.retain(|entry| entry.derived_data_type == ddt);
    }
    entries.sort_by_key(|entry| (entry.repo_id, entry.derived_data_type));

    let mut table = Table::new();
    table.add_row(row!["Repo ID", "Derived Data Type", "Root Request ID"]);
    for entry in entries {
        let repo_id = entry.repo_id.id().to_string();
        let ddt = entry.derived_data_type.name().to_string();
        let root_request_id = match entry.root_request_id {
            Some(id) => id.to_string(),
            None => "NULL".to_string(),
        };
        table.add_row(row![repo_id, ddt, root_request_id]);
    }
    table.printstd();

    Ok(())
}
