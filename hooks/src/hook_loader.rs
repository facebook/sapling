// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! This sub module contains functions to load hooks for the server

#![deny(warnings)]

use super::HookManager;
use super::lua_hook::LuaHook;
use bookmarks::Bookmark;
use failure::Error;
use metaconfig::repoconfig::{HookType, RepoConfig};
use std::collections::HashSet;
use std::sync::Arc;

pub fn load_hooks(hook_manager: &mut HookManager, config: RepoConfig) -> Result<(), Error> {
    match config.hooks {
        Some(hooks) => {
            let mut hook_set = HashSet::new();
            for hook in hooks {
                let name = hook.name;
                let lua_hook = LuaHook::new(name.clone(), hook.code.clone());
                match hook.hook_type {
                    HookType::PerFile => hook_manager.register_file_hook(&name, Arc::new(lua_hook)),
                    HookType::PerChangeset => {
                        hook_manager.register_changeset_hook(&name, Arc::new(lua_hook))
                    }
                }
                hook_set.insert(name);
            }
            match config.bookmarks {
                Some(bookmarks) => for bookmark_hook in bookmarks {
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
                },
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
}

#[cfg(test)]
mod test {
    use super::*;
    use super::ErrorKind;
    use super::super::*;
    use async_unit;
    use fixtures::many_files_dirs;
    use metaconfig::repoconfig::{BookmarkParams, HookParams, RepoType};
    use slog::{Discard, Drain};

    #[test]
    fn test_load_hooks() {
        async_unit::tokio_unit_test(|| {
            let config = RepoConfig {
                repotype: RepoType::Revlog("whatev".into()),
                enabled: true,
                generation_cache_size: 1,
                repoid: 1,
                scuba_table: None,
                cache_warmup: None,
                bookmarks: Some(vec![
                    BookmarkParams {
                        bookmark: Bookmark::new("bm1").unwrap(),
                        hooks: Some(vec!["hook1".into(), "hook2".into()]),
                    },
                    BookmarkParams {
                        bookmark: Bookmark::new("bm2").unwrap(),
                        hooks: Some(vec!["hook2".into(), "hook3".into()]),
                    },
                ]),
                hooks: Some(vec![
                    HookParams {
                        name: "hook1".into(),
                        code: "hook1 code".into(),
                        hook_type: HookType::PerFile,
                    },
                    HookParams {
                        name: "hook2".into(),
                        code: "hook2 code".into(),
                        hook_type: HookType::PerFile,
                    },
                    HookParams {
                        name: "hook3".into(),
                        code: "hook3 code".into(),
                        hook_type: HookType::PerChangeset,
                    },
                ]),
                pushrebase: Default::default(),
            };

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
            let config = RepoConfig {
                repotype: RepoType::Revlog("whatev".into()),
                enabled: true,
                generation_cache_size: 1,
                repoid: 1,
                scuba_table: None,
                cache_warmup: None,
                bookmarks: Some(vec![
                    BookmarkParams {
                        bookmark: Bookmark::new("bm1").unwrap(),
                        hooks: Some(vec!["hook1".into(), "hook2".into()]),
                    },
                ]),
                hooks: Some(vec![
                    HookParams {
                        name: "hook1".into(),
                        code: "hook1 code".into(),
                        hook_type: HookType::PerFile,
                    },
                ]),
                pushrebase: Default::default(),
            };

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

    fn hook_manager_blobrepo() -> HookManager {
        let repo = many_files_dirs::getrepo(None);
        let logger = Logger::root(Discard {}.ignore_res(), o!());
        HookManager::new_with_blobrepo(repo, logger)
    }

}
