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
#![feature(try_from)]

pub extern crate lua52_sys as ffi;

#[cfg(test)]
#[macro_use]
extern crate assert_matches;
#[cfg(test)]
extern crate async_unit;
extern crate asyncmemo;
extern crate blobrepo;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
extern crate futures_ext;
extern crate hlua;
extern crate hlua_futures;
#[cfg(test)]
extern crate linear;
#[macro_use]
extern crate maplit;
extern crate mercurial_types;
#[cfg(test)]
extern crate tokio_core;

pub mod lua_hook;
pub mod rust_hook;

use asyncmemo::{Asyncmemo, Filler, Weight};
use blobrepo::{BlobChangeset, BlobRepo};
use failure::Error;
use futures::Future;
use futures_ext::{BoxFuture, FutureExt};
use mercurial_types::{Changeset, HgChangesetId, HgParents};
use std::collections::HashMap;
use std::collections::hash_map::IntoIter;
use std::convert::TryFrom;
use std::mem;
use std::str;
use std::sync::{Arc, Mutex};
type Hooks = Arc<Mutex<HashMap<String, Arc<Hook>>>>;

/// Manages hooks and allows them to be installed and uninstalled given a name
/// Knows how to run hooks
pub struct HookManager {
    cache: Asyncmemo<HookCacheFiller>,
    hooks: Hooks,
}

/// Represents the status of a (non error) hook run
#[derive(Clone, Debug, PartialEq)]
pub enum HookExecution {
    Accepted,
    Rejected(HookRejectionInfo),
}

impl Weight for HookExecution {
    fn get_weight(&self) -> usize {
        match self {
            HookExecution::Accepted => mem::size_of::<Self>(),
            HookExecution::Rejected(info) => mem::size_of::<Self>() + info.get_weight(),
        }
    }
}

/// Information on why the hook rejected the changeset
#[derive(Clone, Debug, PartialEq)]
pub struct HookRejectionInfo {
    pub description: String,
    pub long_description: String,
    // TODO more fields
}

impl Weight for HookRejectionInfo {
    fn get_weight(&self) -> usize {
        mem::size_of::<Self>() + self.description.get_weight() + self.long_description.get_weight()
    }
}

impl HookRejectionInfo {
    pub fn new(description: String, long_description: String) -> HookRejectionInfo {
        HookRejectionInfo {
            description,
            long_description,
        }
    }
}

struct HookCacheFiller {
    repo_name: String,
    hooks: Hooks,
    repo: BlobRepo,
}

impl Filler for HookCacheFiller {
    type Key = (String, HgChangesetId); // (hook_name, hash)
    type Value = BoxFuture<HookExecution, Error>;

    fn fill(&self, _cache: &Asyncmemo<Self>, key: &Self::Key) -> Self::Value {
        let hook_name = key.0.clone();
        let changeset_id = key.1;
        let hooks = self.hooks.lock().unwrap();
        let repo_name = self.repo_name.clone();
        match hooks.get(&hook_name) {
            Some(arc_hook) => {
                let arc_hook = arc_hook.clone();
                self.repo
                    .get_changeset_by_changesetid(&changeset_id)
                    .then(|res| match res {
                        Ok(cs) => HookChangeset::try_from(cs),
                        Err(e) => Err(e),
                    })
                    .and_then(move |hcs| {
                        let hook_context =
                            HookContext::new(hook_name.clone(), repo_name, Arc::new(hcs));
                        arc_hook.run(hook_context)
                    })
                    .boxify()
            }
            None => panic!("Can't find hook"), // TODO
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct HookExecutionHolder {
    hook_name: String,
    hook_execution: HookExecution,
}

impl HookManager {
    pub fn new(
        repo_name: String,
        repo: BlobRepo,
        entrylimit: usize,
        weightlimit: usize,
    ) -> HookManager {
        let hooks = Arc::new(Mutex::new(HashMap::new()));
        let filler = HookCacheFiller {
            hooks: hooks.clone(),
            repo,
            repo_name,
        };
        let cache = Asyncmemo::with_limits(filler, entrylimit, weightlimit);
        HookManager { cache, hooks }
    }

    pub fn install_hook(&mut self, hook_name: &str, hook: Arc<Hook>) {
        let mut hooks = self.hooks.lock().unwrap();
        hooks.insert(hook_name.to_string(), hook);
    }

    pub fn uninstall_hook(&mut self, hook_name: &str) {
        let mut hooks = self.hooks.lock().unwrap();
        hooks.remove(hook_name);
    }

    pub fn iter(&self) -> IntoIter<String, Arc<Hook>> {
        let hooks = self.hooks.lock().unwrap();
        let cloned = hooks.clone();
        cloned.into_iter()
    }

    pub fn run_hooks(
        &self,
        changeset_id: HgChangesetId,
    ) -> BoxFuture<HashMap<String, HookExecution>, Error> {
        let hooks = self.hooks.lock().unwrap();
        let v: Vec<BoxFuture<HookExecutionHolder, _>> = hooks
            .iter()
            .map(|(hook_name, _)| self.run_hook(hook_name.to_string(), changeset_id.clone()))
            .collect();
        futures::future::join_all(v)
            .map(|v| {
                let mut map = HashMap::new();
                v.iter().for_each(|heh| {
                    map.insert(heh.hook_name.clone(), heh.hook_execution.clone());
                });
                map
            })
            .boxify()
    }

    fn run_hook(
        &self,
        hook_name: String,
        changeset_id: HgChangesetId,
    ) -> BoxFuture<HookExecutionHolder, Error> {
        let hook_name2 = hook_name.clone();
        self.cache
            .get((hook_name.to_string(), changeset_id.clone()))
            .map(move |hook_execution| HookExecutionHolder {
                hook_name: hook_name2,
                hook_execution,
            })
            .boxify()
    }
}

pub trait Hook: Send + Sync {
    fn run(&self, hook_context: HookContext) -> BoxFuture<HookExecution, Error>;
}

/// Represents a changeset - more user friendly than the blob changeset
/// as this uses String not Vec[u8]
pub struct HookChangeset {
    pub author: String,
    pub files: Vec<String>,
    pub comments: String,
    pub parents: HookChangesetParents,
}

impl HookChangeset {
    pub fn new(
        author: String,
        files: Vec<String>,
        comments: String,
        parents: HookChangesetParents,
    ) -> HookChangeset {
        HookChangeset {
            author,
            files,
            comments,
            parents,
        }
    }
}

pub enum HookChangesetParents {
    None,
    One(String),
    Two(String, String),
}

impl TryFrom<BlobChangeset> for HookChangeset {
    type Error = Error;
    fn try_from(changeset: BlobChangeset) -> Result<Self, Error> {
        let author = str::from_utf8(changeset.user())?.into();
        let files = changeset.files();
        let files = files
            .iter()
            .map(|arr| String::from_utf8_lossy(&arr.to_vec()).into_owned())
            .collect();
        let comments = str::from_utf8(changeset.user())?.into();
        let parents = HookChangesetParents::from(changeset.parents());
        Ok(HookChangeset {
            author,
            files,
            comments,
            parents,
        })
    }
}

impl From<HgParents> for HookChangesetParents {
    fn from(parents: HgParents) -> Self {
        match parents {
            HgParents::None => HookChangesetParents::None,
            HgParents::One(p1_hash) => HookChangesetParents::One(p1_hash.to_string()),
            HgParents::Two(p1_hash, p2_hash) => {
                HookChangesetParents::Two(p1_hash.to_string(), p2_hash.to_string())
            }
        }
    }
}

pub struct HookContext {
    pub hook_name: String,
    pub repo_name: String,
    pub changeset: Arc<HookChangeset>,
}

impl HookContext {
    fn new(hook_name: String, repo_name: String, changeset: Arc<HookChangeset>) -> HookContext {
        HookContext {
            hook_name,
            repo_name,
            changeset,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use futures::Future;
    use futures::future::finished;
    use linear;
    use std::collections::HashSet;
    use std::str::FromStr;

    struct TestHook {
        expected_execution: HookExecution,
    }

    impl Hook for TestHook {
        fn run(&self, _: HookContext) -> BoxFuture<HookExecution, Error> {
            finished(self.expected_execution.clone()).boxify()
        }
    }

    #[test]
    fn test_run_hooks() {
        async_unit::tokio_unit_test(|| {
            let mut hook_manager = hook_manager();
            let hook1 = TestHook {
                expected_execution: HookExecution::Accepted,
            };
            let hook1_expected = hook1.expected_execution.clone();
            hook_manager.install_hook("testhook1", Arc::new(hook1));
            let hook2 = TestHook {
                expected_execution: HookExecution::Rejected(HookRejectionInfo::new(
                    "d1".into(),
                    "d2".into(),
                )),
            };
            let hook2_expected = hook2.expected_execution.clone();
            hook_manager.install_hook("testhook2", Arc::new(hook2));
            let change_set_id =
                HgChangesetId::from_str("a5ffa77602a066db7d5cfb9fb5823a0895717c5a").unwrap();
            let fut: BoxFuture<HashMap<String, HookExecution>, Error> =
                hook_manager.run_hooks(change_set_id);
            let res = fut.wait();
            match res {
                Ok(map) => {
                    assert_eq!(map.len(), 2);
                    let hook_execution = map.get("testhook1").unwrap();
                    assert_eq!(hook1_expected, *hook_execution);
                    let hook_execution = map.get("testhook2").unwrap();
                    assert_eq!(hook2_expected, *hook_execution);
                }
                Err(e) => {
                    println!("Failed to run hook {}", e);
                    assert!(false); // Just fail
                }
            }
        });
    }

    #[test]
    fn test_run_twice() {
        async_unit::tokio_unit_test(|| {
            let mut hook_manager = hook_manager();
            let hook1 = TestHook {
                expected_execution: HookExecution::Accepted,
            };
            let hook1_expected = hook1.expected_execution.clone();
            hook_manager.install_hook("testhook1", Arc::new(hook1));

            for _ in 0..2 {
                let change_set_id =
                    HgChangesetId::from_str("a5ffa77602a066db7d5cfb9fb5823a0895717c5a").unwrap();
                let fut: BoxFuture<HashMap<String, HookExecution>, Error> =
                    hook_manager.run_hooks(change_set_id);
                let res = fut.wait();
                match res {
                    Ok(map) => {
                        assert_eq!(map.len(), 1);
                        let hook_execution = map.get("testhook1").unwrap();
                        assert_eq!(hook1_expected, *hook_execution);
                    }
                    Err(e) => {
                        println!("Failed to run hook {}", e);
                        assert!(false); // Just fail
                    }
                }
            }
        });
    }

    #[test]
    fn test_install() {
        async_unit::tokio_unit_test(|| {
            let mut hook_manager = hook_manager();
            let hook1 = TestHook {
                expected_execution: HookExecution::Accepted,
            };
            hook_manager.install_hook("testhook1", Arc::new(hook1));
            let hook2 = TestHook {
                expected_execution: HookExecution::Accepted,
            };
            hook_manager.install_hook("testhook2", Arc::new(hook2));

            let mut set = HashSet::new();
            hook_manager.iter().for_each(|(k, _)| {
                set.insert(k.clone());
            });

            assert_eq!(2, set.len());
            assert!(set.contains("testhook1"));
            assert!(set.contains("testhook2"));
        });
    }

    #[test]
    fn test_uninstall() {
        async_unit::tokio_unit_test(|| {
            let mut hook_manager = hook_manager();
            let hook1 = TestHook {
                expected_execution: HookExecution::Accepted,
            };
            hook_manager.install_hook("testhook1", Arc::new(hook1));
            let hook2 = TestHook {
                expected_execution: HookExecution::Accepted,
            };
            hook_manager.install_hook("testhook2", Arc::new(hook2));

            hook_manager.uninstall_hook("testhook1");

            let mut set = HashSet::new();
            hook_manager.iter().for_each(|(k, _)| {
                set.insert(k.clone());
            });

            assert_eq!(1, set.len());
            assert!(set.contains("testhook2"));
        });
    }

    fn hook_manager() -> HookManager {
        let repo = linear::getrepo(None);
        HookManager::new("some_repo".into(), repo, 1024, 1024 * 1024)
    }

}
