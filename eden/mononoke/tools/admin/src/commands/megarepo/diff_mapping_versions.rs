/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use anyhow::format_err;
use context::CoreContext;
use live_commit_sync_config::LiveCommitSyncConfig;
use megarepolib::commit_sync_config_utils::diff_small_repo_commit_sync_configs;
use metaconfig_types::CommitSyncConfigVersion;
use mononoke_api::Repo;
use mononoke_app::MononokeApp;
use mononoke_app::args::SourceAndTargetRepoArgs;

use super::common::get_live_commit_sync_config;

/// Show difference between two mapping versions
#[derive(Debug, clap::Args)]
pub struct DiffMappingVersionsArgs {
    #[clap(flatten)]
    pub repo_args: SourceAndTargetRepoArgs,

    /// List of mapping versions
    #[clap(long)]
    pub mapping_version_names: Vec<String>,
}

pub async fn run(ctx: &CoreContext, app: MononokeApp, args: DiffMappingVersionsArgs) -> Result<()> {
    if args.mapping_version_names.len() != 2 {
        return Err(format_err!(
            "mapping_version_names should have exactly 2 values"
        ));
    }

    let source_repo: Repo = app.open_repo(&args.repo_args.source_repo).await?;
    let target_repo: Repo = app.open_repo(&args.repo_args.target_repo).await?;
    let source_repo_id = source_repo.repo_identity.id();
    let target_repo_id = target_repo.repo_identity.id();

    let live_commit_sync_config =
        get_live_commit_sync_config(ctx, &app, &args.repo_args.source_repo).await?;

    let mut commit_sync_configs = vec![];
    for version in args.mapping_version_names {
        let version = CommitSyncConfigVersion(version.to_string());
        let config = live_commit_sync_config
            .get_commit_sync_config_by_version(target_repo_id, &version)
            .await?;
        commit_sync_configs.push(config);
    }

    // Validate that both versions related to the same config.
    let from = commit_sync_configs.remove(0);
    let to = commit_sync_configs.remove(0);
    if from.large_repo_id != to.large_repo_id {
        return Err(format_err!(
            "different large repo ids: {} vs {}",
            from.large_repo_id,
            to.large_repo_id
        ));
    }

    let small_repo_id = if from.large_repo_id == target_repo_id {
        source_repo_id
    } else {
        target_repo_id
    };

    if !from.small_repos.contains_key(&small_repo_id) {
        return Err(format_err!(
            "{} doesn't have small repo id {}",
            from.version_name,
            small_repo_id,
        ));
    }

    if !to.small_repos.contains_key(&small_repo_id) {
        return Err(format_err!(
            "{} doesn't have small repo id {}",
            to.version_name,
            small_repo_id,
        ));
    }

    let from_small_commit_sync_config = from
        .small_repos
        .get(&small_repo_id)
        .cloned()
        .ok_or_else(|| format_err!("{} not found in {}", small_repo_id, from.version_name))?;
    let to_small_commit_sync_config = to
        .small_repos
        .get(&small_repo_id)
        .cloned()
        .ok_or_else(|| format_err!("{} not found in {}", small_repo_id, to.version_name))?;

    let diff = diff_small_repo_commit_sync_configs(
        from_small_commit_sync_config,
        to_small_commit_sync_config,
    );

    if let Some((from, to)) = diff.default_action_change {
        println!("default action change: {:?} to {:?}", from, to);
    }

    let mut mapping_added = diff.mapping_added.into_iter().collect::<Vec<_>>();
    mapping_added.sort();
    for (path_from, path_to) in mapping_added {
        println!("mapping added: {} => {}", path_from, path_to);
    }

    let mut mapping_changed = diff.mapping_changed.into_iter().collect::<Vec<_>>();
    mapping_changed.sort();
    for (path_from, (before, after)) in mapping_changed {
        println!("mapping changed: {} => {} vs {}", path_from, before, after);
    }

    let mut mapping_removed = diff.mapping_removed.into_iter().collect::<Vec<_>>();
    mapping_removed.sort();
    for (path_from, path_to) in mapping_removed {
        println!("mapping removed: {} => {}", path_from, path_to);
    }

    Ok(())
}
