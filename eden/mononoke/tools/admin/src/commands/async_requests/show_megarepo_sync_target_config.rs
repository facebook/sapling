/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use clap::Args;
use context::CoreContext;
use megarepo_config::CfgrMononokeMegarepoConfigs;
use megarepo_config::MononokeMegarepoConfigs;
use megarepo_config::SyncConfigVersion;
use megarepo_config::Target;
use metaconfig_types::RepoConfig;
use mononoke_app::MononokeApp;
use mononoke_types::RepositoryId;

#[derive(Args)]
/// Shows the contents of a megarepo SyncTargetConfig by version.
/// Use this to inspect what sources and settings are defined for
/// a particular config version string (as shown in async_requests show).
pub struct AsyncRequestsShowMegarepoSyncTargetConfigArgs {
    /// The bookmark name of the target.
    #[clap(long)]
    bookmark: String,
    /// The config version string to look up.
    #[clap(long)]
    version: String,
}

pub async fn show_megarepo_sync_target_config(
    args: AsyncRequestsShowMegarepoSyncTargetConfigArgs,
    ctx: CoreContext,
    app: &MononokeApp,
    repo_id: RepositoryId,
    repo_config: Arc<RepoConfig>,
) -> Result<(), Error> {
    let env = app.environment();
    let megarepo_configs =
        CfgrMononokeMegarepoConfigs::new(env.fb, env.mysql_options.clone(), env.readonly_storage)
            .context("Failed to create megarepo configs client")?;

    let target = Target {
        repo_id: repo_id.id() as i64,
        bookmark: args.bookmark.clone(),
        ..Default::default()
    };
    let version: SyncConfigVersion = args.version.clone();

    let config = megarepo_configs
        .get_config_by_version(ctx, repo_config, target, version)
        .await
        .context("Failed to get config by version")?;

    println!("SyncTargetConfig:");
    println!("  Target:");
    println!("    repo_id: {}", config.target.repo_id);
    println!("    bookmark: {}", config.target.bookmark);
    println!("  Version: {}", config.version);
    println!("  Sources:");
    for (i, source) in config.sources.iter().enumerate() {
        println!("    [{}] {}", i, source.source_name);
        println!("        repo_id: {}", source.repo_id);
        println!("        revision: {:?}", source.revision);
        println!("        mapping: {:?}", source.mapping);
        println!("        merge_mode: {:?}", source.merge_mode);
    }

    Ok(())
}
