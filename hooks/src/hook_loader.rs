// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! This sub module contains functions to load hooks for the server

#![deny(warnings)]

use super::HookManager;
use super::lua_hook::LuaHook;
use failure::Error;
use metaconfig::repoconfig::RepoConfig;
use std::collections::HashSet;
use std::sync::Arc;

pub fn load_hooks(hook_manager: &mut HookManager, config: RepoConfig) -> Result<(), Error> {
    match config.hooks {
        Some(hooks) => {
            let mut hook_set = HashSet::new();
            for hook in hooks {
                let name = hook.name;
                let hook = LuaHook::new(name.clone(), hook.code.clone());
                hook_manager.install_hook(&name, Arc::new(hook));
                hook_set.insert(name);
            }
            match config.bookmarks {
                Some(bookmarks) => for bookmark_hook in bookmarks {
                    let bm_name = bookmark_hook.name;
                    let hooks = bookmark_hook.hooks;
                    if let Some(hooks) = hooks {
                        let bm_hook_set: HashSet<String> = hooks.clone().into_iter().collect();
                        let diff: HashSet<_> = bm_hook_set.difference(&hook_set).collect();
                        if diff.len() != 0 {
                            return Err(ErrorKind::NoSuchBookmarkHook(bm_name).into());
                        } else {
                            hook_manager.set_hooks_for_bookmark(bm_name, hooks);
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
    NoSuchBookmarkHook(String),
}

#[cfg(test)]
mod test {
    use super::*;
    use super::ErrorKind;
    use super::super::*;
    use async_unit;
    use linear;
    use metaconfig::repoconfig::{BookmarkParams, HookParams, RepoType};

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
                        name: "bm1".into(),
                        hooks: Some(vec!["hook1".into(), "hook2".into()]),
                    },
                    BookmarkParams {
                        name: "bm2".into(),
                        hooks: Some(vec!["hook2".into(), "hook3".into()]),
                    },
                ]),
                hooks: Some(vec![
                    HookParams {
                        name: "hook1".into(),
                        code: "hook1 code".into(),
                    },
                    HookParams {
                        name: "hook2".into(),
                        code: "hook2 code".into(),
                    },
                    HookParams {
                        name: "hook3".into(),
                        code: "hook3 code".into(),
                    },
                ]),
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
                        name: "bm1".into(),
                        hooks: Some(vec!["hook1".into(), "hook2".into()]),
                    },
                ]),
                hooks: Some(vec![
                    HookParams {
                        name: "hook1".into(),
                        code: "hook1 code".into(),
                    },
                ]),
            };

            let mut hm = hook_manager_blobrepo();

            match load_hooks(&mut hm, config)
                .unwrap_err()
                .downcast::<ErrorKind>()
            {
                Ok(ErrorKind::NoSuchBookmarkHook(bm_name)) => {
                    assert_eq!("bm1", bm_name);
                }
                _ => assert!(false, "Unexpected err type"),
            };
        });
    }

    fn hook_manager_blobrepo() -> HookManager {
        let repo = linear::getrepo(None);
        let store = BlobRepoChangesetStore { repo };
        HookManager::new("some_repo".into(), Box::new(store), 1024, 1024 * 1024)
    }

}
