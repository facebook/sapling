/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! This sub module contains functions to load hooks for the server

use crate::errors::*;
use crate::ChangesetHook;
use crate::FileHook;
use crate::HookManager;
use anyhow::Error;
use fbinit::FacebookInit;
use metaconfig_types::RepoConfig;
use permission_checker::AclProvider;
use std::collections::HashSet;

#[cfg(fbcode_build)]
use crate::facebook::rust_hooks::hook_name_to_changeset_hook;
#[cfg(fbcode_build)]
use crate::facebook::rust_hooks::hook_name_to_file_hook;
#[cfg(not(fbcode_build))]
use crate::rust_hooks::hook_name_to_changeset_hook;
#[cfg(not(fbcode_build))]
use crate::rust_hooks::hook_name_to_file_hook;

enum LoadedRustHook {
    ChangesetHook(Box<dyn ChangesetHook>),
    FileHook(Box<dyn FileHook>),
}

pub async fn load_hooks(
    fb: FacebookInit,
    acl_provider: &dyn AclProvider,
    hook_manager: &mut HookManager,
    config: &RepoConfig,
    disabled_hooks: &HashSet<String>,
) -> Result<(), Error> {
    let mut hooks_not_disabled = disabled_hooks.clone();

    let mut hook_set = HashSet::new();
    for hook in config.hooks.clone() {
        use LoadedRustHook::*;

        if disabled_hooks.contains(&hook.name) {
            hooks_not_disabled.remove(&hook.name);
            continue;
        }

        let rust_hook = {
            if let Some(hook) = hook_name_to_changeset_hook(
                fb,
                &hook.name,
                &hook.config,
                acl_provider,
                hook_manager.get_reviewers_perm_checker(),
                hook_manager.repo_name(),
            )
            .await?
            {
                ChangesetHook(hook)
            } else if let Some(hook) = hook_name_to_file_hook(fb, &hook.name, &hook.config)? {
                FileHook(hook)
            } else {
                return Err(ErrorKind::InvalidRustHook(hook.name.clone()).into());
            }
        };

        match rust_hook {
            FileHook(rust_hook) => {
                hook_manager.register_file_hook(&hook.name, rust_hook, hook.config)
            }
            ChangesetHook(rust_hook) => {
                hook_manager.register_changeset_hook(&hook.name, rust_hook, hook.config)
            }
        }

        hook_set.insert(hook.name.clone());
    }

    if !hooks_not_disabled.is_empty() {
        return Err(ErrorKind::NoSuchHookToDisable(hooks_not_disabled).into());
    }

    for bookmark_hook in config.bookmarks.clone() {
        let bookmark = bookmark_hook.bookmark;
        let hooks: Vec<_> = bookmark_hook
            .hooks
            .into_iter()
            .filter(|h| !disabled_hooks.contains(h))
            .collect();
        let bm_hook_set: HashSet<String> = hooks.clone().into_iter().collect();
        let diff: HashSet<_> = bm_hook_set.difference(&hook_set).collect();
        if !diff.is_empty() {
            return Err(ErrorKind::NoSuchBookmarkHook(
                bookmark,
                diff.into_iter().cloned().collect(),
            )
            .into());
        } else {
            hook_manager.set_hooks_for_bookmark(bookmark, hooks);
        }
    }

    Ok(())
}
