// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! This sub module contains functions to load hooks for the server

#![deny(warnings)]

use super::lua_hook::LuaHook;
use super::{Hook, HookChangeset, HookManager};
use bookmarks::Bookmark;
use facebook::rust_hooks::ensure_valid_email::EnsureValidEmailHook;
use facebook::rust_hooks::verify_integrity::VerifyIntegrityHook;
use failure::Error;
use metaconfig_types::{HookType, RepoConfig};
use std::collections::HashSet;
use std::sync::Arc;

pub fn load_hooks(hook_manager: &mut HookManager, config: RepoConfig) -> Result<(), Error> {
    match config.hooks {
        Some(hooks) => {
            let mut hook_set = HashSet::new();
            for hook in hooks {
                let name = hook.name;
                if name.starts_with("rust:") {
                    let rust_name = &name[5..];
                    let rust_name = rust_name.to_string();
                    let rust_hook: Arc<Hook<HookChangeset>> = match rust_name.as_ref() {
                        "verify_integrity" => Arc::new(VerifyIntegrityHook::new()),
                        "ensure_valid_email" => Arc::new(EnsureValidEmailHook::new()),
                        _ => return Err(ErrorKind::InvalidRustHook(name.clone()).into()),
                    };
                    hook_manager.register_changeset_hook(&name, rust_hook, hook.config)
                } else {
                    let lua_hook = LuaHook::new(name.clone(), hook.code.clone().unwrap());
                    match hook.hook_type {
                        HookType::PerAddedOrModifiedFile => {
                            hook_manager.register_file_hook(&name, Arc::new(lua_hook), hook.config)
                        }
                        HookType::PerChangeset => hook_manager.register_changeset_hook(
                            &name,
                            Arc::new(lua_hook),
                            hook.config,
                        ),
                    }
                }
                hook_set.insert(name);
            }
            match config.bookmarks {
                Some(bookmarks) => {
                    for bookmark_hook in bookmarks {
                        let bookmark = bookmark_hook.bookmark;
                        let hooks = bookmark_hook.hooks;
                        if let Some(hooks) = hooks {
                            let bm_hook_set: HashSet<String> = hooks.clone().into_iter().collect();
                            let diff: HashSet<_> = bm_hook_set.difference(&hook_set).collect();
                            if diff.len() != 0 {
                                return Err(ErrorKind::NoSuchBookmarkHook(bookmark).into());
                            } else {
                                hook_manager.set_hooks_for_bookmark(bookmark, hooks);
                            }
                        };
                    }
                }
                None => (),
            }
            Ok(())
        }
        None => Ok(()),
    }
}

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "Hook(s) referenced in bookmark {} do not exist", _0)]
    NoSuchBookmarkHook(Bookmark),

    #[fail(display = "invalid rust hook: {}", _0)]
    InvalidRustHook(String),
}

#[cfg(test)]
mod test {
    use super::super::*;
    use super::ErrorKind;
    use super::*;
    use async_unit;
    use context::CoreContext;
    use fixtures::many_files_dirs;
    use metaconfig_types::{BookmarkParams, HookParams, RepoReadOnly, RepoType};
    use slog::{Discard, Drain};

    fn default_repo_config() -> RepoConfig {
        RepoConfig {
            repotype: RepoType::BlobFiles("whatev".into()),
            enabled: true,
            generation_cache_size: 1,
            repoid: 1,
            scuba_table: None,
            cache_warmup: None,
            hook_manager_params: None,
            bookmarks: None,
            hooks: None,
            pushrebase: Default::default(),
            lfs: Default::default(),
            wireproto_scribe_category: None,
            hash_validation_percentage: 0,
            readonly: RepoReadOnly::ReadWrite,
            skiplist_index_blobstore_key: None,
        }
    }

    #[test]
    fn test_load_hooks() {
        async_unit::tokio_unit_test(|| {
            let mut config = default_repo_config();
            config.bookmarks = Some(vec![
                BookmarkParams {
                    bookmark: Bookmark::new("bm1").unwrap(),
                    hooks: Some(vec!["hook1".into(), "hook2".into()]),
                },
                BookmarkParams {
                    bookmark: Bookmark::new("bm2").unwrap(),
                    hooks: Some(vec![
                        "hook2".into(),
                        "hook3".into(),
                        "rust:verify_integrity".into(),
                    ]),
                },
            ]);

            config.hooks = Some(vec![
                HookParams {
                    name: "hook1".into(),
                    code: Some("hook1 code".into()),
                    hook_type: HookType::PerAddedOrModifiedFile,
                    config: Default::default(),
                },
                HookParams {
                    name: "hook2".into(),
                    code: Some("hook2 code".into()),
                    hook_type: HookType::PerAddedOrModifiedFile,
                    config: Default::default(),
                },
                HookParams {
                    name: "hook3".into(),
                    code: Some("hook3 code".into()),
                    hook_type: HookType::PerChangeset,
                    config: Default::default(),
                },
                HookParams {
                    name: "rust:verify_integrity".into(),
                    code: Some("whateva".into()),
                    hook_type: HookType::PerChangeset,
                    config: Default::default(),
                },
            ]);

            let mut hm = hook_manager_blobrepo();
            match load_hooks(&mut hm, config) {
                Err(e) => assert!(false, format!("Failed to load hooks {}", e)),
                Ok(()) => (),
            };
        });
    }

    #[test]
    fn test_load_hooks_no_such_hook() {
        async_unit::tokio_unit_test(|| {
            let mut config = default_repo_config();
            config.bookmarks = Some(vec![
                BookmarkParams {
                    bookmark: Bookmark::new("bm1").unwrap(),
                    hooks: Some(vec!["hook1".into(), "hook2".into()]),
                },
            ]);

            config.hooks = Some(vec![
                HookParams {
                    name: "hook1".into(),
                    code: Some("hook1 code".into()),
                    hook_type: HookType::PerAddedOrModifiedFile,
                    config: Default::default(),
                },
            ]);

            let mut hm = hook_manager_blobrepo();

            match load_hooks(&mut hm, config)
                .unwrap_err()
                .downcast::<ErrorKind>()
            {
                Ok(ErrorKind::NoSuchBookmarkHook(bookmark)) => {
                    assert_eq!(Bookmark::new("bm1").unwrap(), bookmark);
                }
                _ => assert!(false, "Unexpected err type"),
            };
        });
    }

    #[test]
    fn test_load_hooks_bad_rust_hook() {
        async_unit::tokio_unit_test(|| {
            let mut config = default_repo_config();
            config.bookmarks = Some(vec![
                BookmarkParams {
                    bookmark: Bookmark::new("bm1").unwrap(),
                    hooks: Some(vec!["rust:hook1".into()]),
                },
            ]);

            config.hooks = Some(vec![
                HookParams {
                    name: "rust:hook1".into(),
                    code: Some("hook1 code".into()),
                    hook_type: HookType::PerChangeset,
                    config: Default::default(),
                },
            ]);

            let mut hm = hook_manager_blobrepo();

            match load_hooks(&mut hm, config)
                .unwrap_err()
                .downcast::<ErrorKind>()
            {
                Ok(ErrorKind::InvalidRustHook(hook_name)) => {
                    assert_eq!(hook_name, "rust:hook1".to_string());
                }
                _ => assert!(false, "Unexpected err type"),
            };
        });
    }

    fn hook_manager_blobrepo() -> HookManager {
        let ctx = CoreContext::test_mock();
        let repo = many_files_dirs::getrepo(None);
        let logger = Logger::root(Discard {}.ignore_res(), o!());
        HookManager::new_with_blobrepo(ctx, Default::default(), repo, logger)
    }

}
