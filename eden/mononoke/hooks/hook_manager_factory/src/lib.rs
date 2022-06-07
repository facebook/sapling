/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;

use anyhow::Result;
use blobrepo::BlobRepo;
use context::CoreContext;
use hooks::hook_loader::load_hooks;
use hooks::HookManager;
use hooks_content_stores::blobrepo_text_only_fetcher;
use metaconfig_types::RepoConfig;
use scuba_ext::MononokeScubaSampleBuilder;

pub async fn make_hook_manager(
    ctx: &CoreContext,
    repo: &BlobRepo,
    config: &RepoConfig,
    name: &str,
    disabled_hooks: &HashSet<String>,
) -> Result<HookManager> {
    let hook_max_file_size = config.hook_max_file_size.clone();
    let hooks_scuba_table = config.scuba_table_hooks.clone();
    let hooks_scuba_local_path = config.scuba_local_path_hooks.clone();
    let mut hooks_scuba = MononokeScubaSampleBuilder::with_opt_table(ctx.fb, hooks_scuba_table);
    hooks_scuba.add("repo", name.to_string());
    if let Some(hooks_scuba_local_path) = hooks_scuba_local_path {
        hooks_scuba = hooks_scuba.with_log_file(hooks_scuba_local_path)?;
    }
    let hook_manager_params = config.hook_manager_params.clone();

    let mut hook_manager = HookManager::new(
        ctx.fb,
        blobrepo_text_only_fetcher(repo.clone(), hook_max_file_size),
        hook_manager_params.unwrap_or_default(),
        hooks_scuba,
        repo.name().clone(),
    )
    .await?;

    load_hooks(ctx.fb, &mut hook_manager, config, disabled_hooks).await?;

    Ok(hook_manager)
}
