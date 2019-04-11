// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! This sub module contains functions to load hooks for the server

#![deny(warnings)]

use crate::errors::*;
use crate::facebook::rust_hooks::check_unittests::CheckUnittestsHook;
use crate::facebook::rust_hooks::ensure_valid_email::EnsureValidEmailHook;
use crate::facebook::rust_hooks::restrict_users::RestrictUsersHook;
use crate::facebook::rust_hooks::verify_integrity::VerifyIntegrityHook;
use crate::lua_hook::LuaHook;
use crate::{Hook, HookChangeset, HookManager};
use failure::Error;
use metaconfig_types::{HookType, RepoConfig};
use std::collections::HashSet;
use std::sync::Arc;

pub fn load_hooks(hook_manager: &mut HookManager, config: RepoConfig) -> Result<(), Error> {
    let mut hook_set = HashSet::new();
    for hook in config.hooks {
        let name = hook.name;
        if name.starts_with("rust:") {
            let rust_name = &name[5..];
            let rust_name = rust_name.to_string();
            let rust_hook: Arc<Hook<HookChangeset>> = match rust_name.as_ref() {
                "check_unittests" => Arc::new(CheckUnittestsHook::new(&hook.config)?),
                "verify_integrity" => Arc::new(VerifyIntegrityHook::new(&hook.config)?),
                "ensure_valid_email" => Arc::new(EnsureValidEmailHook::new(&hook.config)),
                "restrict_users" => Arc::new(RestrictUsersHook::new(&hook.config)?),
                _ => return Err(ErrorKind::InvalidRustHook(name.clone()).into()),
            };
            hook_manager.register_changeset_hook(&name, rust_hook, hook.config)
        } else {
            let lua_hook = LuaHook::new(name.clone(), hook.code.clone().unwrap());
            match hook.hook_type {
                HookType::PerAddedOrModifiedFile => {
                    hook_manager.register_file_hook(&name, Arc::new(lua_hook), hook.config)
                }
                HookType::PerChangeset => {
                    hook_manager.register_changeset_hook(&name, Arc::new(lua_hook), hook.config)
                }
            }
        }
        hook_set.insert(name);
    }
    for bookmark_hook in config.bookmarks {
        let bookmark = bookmark_hook.bookmark;
        let hooks = bookmark_hook.hooks;
        let bm_hook_set: HashSet<String> = hooks.clone().into_iter().collect();
        let diff: HashSet<_> = bm_hook_set.difference(&hook_set).collect();
        if diff.len() != 0 {
            return Err(ErrorKind::NoSuchBookmarkHook(bookmark).into());
        } else {
            hook_manager.set_hooks_for_bookmark(bookmark, hooks);
        }
    }
    Ok(())
}
