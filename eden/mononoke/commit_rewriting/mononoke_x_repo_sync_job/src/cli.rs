/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::command;
use clap::Args;
use clap::Parser;
use cmdlib_logging::ScubaLoggingArgs;
use fbinit::FacebookInit;
use mononoke_app::args::ChangesetArgs;
use mononoke_app::args::SourceAndTargetRepoArgs;
use mononoke_app::fb303::Fb303AppExtension;
use mononoke_app::MononokeApp;
use mononoke_app::MononokeAppBuilder;

use crate::reporting::SCUBA_TABLE;

#[derive(Debug, Args)]
#[clap(about = "Import all commits from a small repo into a large one before setting up live sync")]
pub struct InitialImportCommandArgs {
    #[clap(long = "version-name")]
    pub sync_config_version_name: String,

    #[clap(flatten)]
    pub changeset_args: ChangesetArgs,

    #[clap(long)]
    pub no_progress_bar: bool,
}

#[derive(Debug, Args)]
#[clap(
    about = "Sync a commit and all of its unsynced ancestors if the given commit has at least one synced ancestor"
)]
pub struct OnceCommandArgs {
    #[clap(long)]
    pub target_bookmark: Option<String>,
    #[clap(long = "commit", short = 'i')]
    pub commit: String,
    // Performs mapping version change.
    #[clap(long = "unsafe-change-version-to")]
    pub new_version: Option<String>,
}

#[derive(Debug, Args, Clone)]
#[clap(
    about = "Start a live sync between repos, so commits from the small repo are automatically synced to the large one"
)]
pub struct TailCommandArgs {
    #[clap(long, default_value_t = 10)]
    pub sleep_secs: u64,
    #[clap(long)]
    pub catch_up_once: bool,
    #[clap(long, required = false)]
    pub backsync_pressure_repo_ids: Vec<i32>,
    #[clap(long)]
    pub hg_sync_backpressure: bool,
    #[clap(long, required = false)]
    pub derived_data_types: Vec<String>,
    #[clap(long)]
    pub bookmark_regex: Option<String>,
}

#[derive(Debug, clap::Subcommand)]
pub enum ForwardSyncerCommand {
    #[command()]
    InitialImport(InitialImportCommandArgs),
    #[command()]
    Once(OnceCommandArgs),
    #[command()]
    Tail(TailCommandArgs),
}

#[derive(Debug, Parser)]
#[clap(about = "CLI to sync commits from small repositories to large ones (i.e. mega repos)")]
pub struct ForwardSyncerArgs {
    /// Identifiers or names for the source and target repos
    #[clap(flatten, next_help_heading = "CROSS REPO OPTIONS")]
    pub repo_args: SourceAndTargetRepoArgs,

    #[clap(long)]
    pub pushrebase_rewrite_dates: bool,

    #[command(subcommand)]
    pub command: ForwardSyncerCommand,

    // TODO(gustavoavena): remove the default scuba logging after all tw
    // tasks have been updated.
    /// Temporary argument that does nothing but is needed to maintain backwards
    /// compatibility.
    #[clap(long)]
    pub log_to_scuba: bool,
}

pub fn create_app(fb: FacebookInit) -> Result<MononokeApp> {
    // TODO(gustavoavena): remove the default scuba logging after all tw
    // tasks have been updated.
    let default_scuba_logging_args = ScubaLoggingArgs {
        scuba_dataset: Some(SCUBA_TABLE.to_string()),
        no_default_scuba_dataset: false,
        warm_bookmark_cache_scuba_dataset: None,
    };
    let app: MononokeApp = MononokeAppBuilder::new(fb)
        .with_arg_defaults(default_scuba_logging_args)
        .with_app_extension(Fb303AppExtension {})
        .build::<ForwardSyncerArgs>()?;

    Ok(app)
}
