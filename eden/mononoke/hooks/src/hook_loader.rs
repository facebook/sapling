/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! This sub module contains functions to load hooks for the server

#![deny(warnings)]

use crate::errors::*;
use crate::{ChangesetHook, FileHook, HookManager};
use anyhow::Error;
use fbinit::FacebookInit;
use metaconfig_types::RepoConfig;
use std::collections::HashSet;

#[cfg(fbcode_build)]
use crate::facebook::rust_hooks::{hook_name_to_changeset_hook, hook_name_to_file_hook};
#[cfg(not(fbcode_build))]
use crate::rust_hooks::{hook_name_to_changeset_hook, hook_name_to_file_hook};

enum LoadedRustHook {
    ChangesetHook(Box<dyn ChangesetHook>),
    FileHook(Box<dyn FileHook>),
}

pub fn load_hooks(
    fb: FacebookInit,
    hook_manager: &mut HookManager,
    config: RepoConfig,
    disabled_hooks: &HashSet<String>,
) -> Result<(), Error> {
    let mut hooks_not_disabled = disabled_hooks.clone();

    let mut hook_set = HashSet::new();
    for hook in config.hooks {
        use LoadedRustHook::*;
        let name = hook.name;

        if disabled_hooks.contains(&name) {
            hooks_not_disabled.remove(&name);
            continue;
        }

        // Backwards compatibility only
        let hook_name = if name.starts_with("rust:") {
            name[5..].to_string()
        } else {
            name.clone()
        };

        let rust_hook = {
            if let Some(hook) = hook_name_to_changeset_hook(
                fb,
                &hook_name,
                &hook.config,
                hook_manager.get_reviewers_perm_checker(),
            )? {
                ChangesetHook(hook)
            } else if let Some(hook) = hook_name_to_file_hook(&hook_name, &hook.config)? {
                FileHook(hook)
            } else {
                return Err(ErrorKind::InvalidRustHook(name).into());
            }
        };

        match rust_hook {
            FileHook(rust_hook) => hook_manager.register_file_hook(&name, rust_hook, hook.config),
            ChangesetHook(rust_hook) => {
                hook_manager.register_changeset_hook(&name, rust_hook, hook.config)
            }
        }

        hook_set.insert(name);
    }

    if hooks_not_disabled.len() > 0 {
        return Err(ErrorKind::NoSuchHookToDisable(hooks_not_disabled).into());
    }

    for bookmark_hook in config.bookmarks {
        let bookmark = bookmark_hook.bookmark;
        let hooks: Vec<_> = bookmark_hook
            .hooks
            .into_iter()
            .filter(|h| !disabled_hooks.contains(h))
            .collect();
        let bm_hook_set: HashSet<String> = hooks.clone().into_iter().collect();
        let diff: HashSet<_> = bm_hook_set.difference(&hook_set).collect();
        if diff.len() != 0 {
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
