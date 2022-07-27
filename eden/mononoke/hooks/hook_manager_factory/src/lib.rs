/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;

use anyhow::Result;
use fbinit::FacebookInit;
use hooks::hook_loader::load_hooks;
use hooks::HookManager;
use hooks_content_stores::RepoFileContentManager;
use hooks_content_stores::TextOnlyFileContentManager;
use metaconfig_types::RepoConfig;
use permission_checker::AclProvider;
use scuba_ext::MononokeScubaSampleBuilder;

pub async fn make_hook_manager(
    fb: FacebookInit,
    acl_provider: &dyn AclProvider,
    hook_file_content_store: RepoFileContentManager,
    config: &RepoConfig,
    name: String,
    disabled_hooks: &HashSet<String>,
) -> Result<HookManager> {
    let hook_max_file_size = config.hook_max_file_size.clone();
    let hooks_scuba_table = config.scuba_table_hooks.clone();
    let hooks_scuba_local_path = config.scuba_local_path_hooks.clone();
    let mut hooks_scuba = MononokeScubaSampleBuilder::with_opt_table(fb, hooks_scuba_table);
    hooks_scuba.add("repo", name.clone());
    if let Some(hooks_scuba_local_path) = hooks_scuba_local_path {
        hooks_scuba = hooks_scuba.with_log_file(hooks_scuba_local_path)?;
    }
    let hook_manager_params = config.hook_manager_params.clone();

    let fetcher = Box::new(TextOnlyFileContentManager::new(
        hook_file_content_store,
        hook_max_file_size,
    ));

    let mut hook_manager = HookManager::new(
        fb,
        acl_provider,
        fetcher,
        hook_manager_params.unwrap_or_default(),
        hooks_scuba,
        name,
    )
    .await?;

    load_hooks(fb, acl_provider, &mut hook_manager, config, disabled_hooks).await?;

    Ok(hook_manager)
}
