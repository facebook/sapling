// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! This crate contains the core structs and traits that implement the hook subsystem in
//! Mononoke.
//! Hooks are user defined pieces of code, typically written in a scripting language that
//! can be run at different stages of the process of rebasing user changes into a server side
//! bookmark.
//! The scripting language specific implementation of hooks are in the corresponding sub module.

#![deny(warnings)]

#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
extern crate futures_ext;
extern crate hlua;
extern crate hlua_futures;
#[macro_use]
extern crate maplit;

#[cfg(test)]
extern crate async_unit;
#[cfg(test)]
extern crate linear;
#[cfg(test)]
extern crate tempdir;
#[cfg(test)]
extern crate tokio_core;

pub mod lua_hook;
pub mod rust_hook;

use failure::Error;
use futures::Future;
use futures_ext::{BoxFuture, FutureExt};
use std::collections::HashMap;
use std::collections::hash_map::Iter;
use std::sync::Arc;

/// Manages hooks and allows them to be installed and uninstalled given a name
/// Knows how to run hooks
pub struct HookManager {
    hooks: HashMap<String, Arc<Hook>>,
}

/// Represents the result of running a hook
pub struct HookResult {
    pub hook_name: String,
    pub passed: bool,
}

impl HookManager {
    pub fn new() -> HookManager {
        HookManager {
            hooks: HashMap::new(),
        }
    }

    pub fn install_hook(&mut self, hook_name: &str, hook: Arc<Hook>) {
        self.hooks.insert(hook_name.to_string(), hook);
    }

    pub fn uninstall_hook(&mut self, hook_name: &str) {
        self.hooks.remove(hook_name);
    }

    pub fn iter(&self) -> Iter<String, Arc<Hook>> {
        self.hooks.iter()
    }

    pub fn run_hooks(&self, changeset: Arc<HookChangeset>) -> BoxFuture<Vec<HookResult>, Error> {
        // Run all hooks, potentially in parallel (depending on hook implementation)
        let v: Vec<BoxFuture<HookResult, _>> = self.hooks
            .iter()
            .map(|(hook_name, hook)| {
                let hook = hook.clone();
                let changeset = changeset.clone();
                let hook_name = hook_name.clone();
                let hook_context = HookContext::new(hook_name.clone(), changeset);
                hook.run(hook_context)
                    .map(move |passed| HookResult {
                        hook_name: hook_name,
                        passed,
                    })
                    .boxify()
            })
            .collect();
        futures::future::join_all(v).boxify()
    }
}

pub trait Hook: Send + Sync {
    fn run(&self, hook_context: HookContext) -> BoxFuture<bool, Error>;
}

/// Represents a changeset - more user friendly than the blob changeset
/// as this uses String not Vec[u8]
pub struct HookChangeset {
    pub author: String,
    pub files: Vec<String>,
}

impl HookChangeset {
    pub fn new(author: String, files: Vec<String>) -> HookChangeset {
        HookChangeset { author, files }
    }
}

pub struct HookContext {
    pub hook_name: String,
    pub changeset: Arc<HookChangeset>,
}

impl HookContext {
    fn new(hook_name: String, changeset: Arc<HookChangeset>) -> HookContext {
        HookContext {
            hook_name,
            changeset,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use futures::Future;
    use futures::future::finished;
    use std::collections::HashSet;

    struct TestHook {
        should_pass: bool,
    }

    impl Hook for TestHook {
        fn run(&self, _: HookContext) -> BoxFuture<bool, Error> {
            finished(self.should_pass).boxify()
        }
    }

    #[test]
    fn test_run_hooks() {
        async_unit::tokio_unit_test(|| {
            let mut hook_manager = hook_manager();
            let hook1 = TestHook { should_pass: true };
            hook_manager.install_hook("testhook1", Arc::new(hook1));
            let hook2 = TestHook { should_pass: false };
            hook_manager.install_hook("testhook2", Arc::new(hook2));
            let author = String::from("jane bloggs");
            let files = vec![String::from("filec")];
            let change_set = HookChangeset::new(author, files);
            let fut: BoxFuture<Vec<HookResult>, Error> =
                hook_manager.run_hooks(Arc::new(change_set));
            let res = fut.wait();
            match res {
                Ok(vec) => {
                    let mut map: HashMap<String, bool> = HashMap::new();
                    vec.into_iter().for_each(|hr| {
                        map.insert(hr.hook_name, hr.passed);
                    });
                    assert_eq!(map.len(), 2);
                    assert!(map.get("testhook1").unwrap());
                    assert!(!map.get("testhook2").unwrap());
                }
                Err(e) => {
                    println!("Failed to run hook {}", e);
                    assert!(false); // Just fail
                }
            }
        });
    }

    #[test]
    fn test_install() {
        let mut hook_manager = hook_manager();
        let hook1 = TestHook { should_pass: true };
        hook_manager.install_hook("testhook1", Arc::new(hook1));
        let hook2 = TestHook { should_pass: true };
        hook_manager.install_hook("testhook2", Arc::new(hook2));

        let mut set = HashSet::new();
        hook_manager.iter().for_each(|(k, _)| {
            set.insert(k.clone());
        });

        assert_eq!(2, set.len());
        assert!(set.contains("testhook1"));
        assert!(set.contains("testhook2"));
    }

    #[test]
    fn test_uninstall() {
        let mut hook_manager = hook_manager();
        let hook1 = TestHook { should_pass: true };
        hook_manager.install_hook("testhook1", Arc::new(hook1));
        let hook2 = TestHook { should_pass: true };
        hook_manager.install_hook("testhook2", Arc::new(hook2));

        hook_manager.uninstall_hook("testhook1");

        let mut set = HashSet::new();
        hook_manager.iter().for_each(|(k, _)| {
            set.insert(k.clone());
        });

        assert_eq!(1, set.len());
        assert!(set.contains("testhook2"));
    }

    fn hook_manager() -> HookManager {
        HookManager::new()
    }

}
