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
#![feature(iterator_flatten)]

#[cfg(test)]
#[macro_use]
extern crate assert_matches;
#[cfg(test)]
extern crate async_unit;
extern crate asyncmemo;
extern crate blobrepo;
extern crate bookmarks;
extern crate bytes;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
#[macro_use]
extern crate futures_ext;
extern crate hlua;
extern crate hlua_futures;
#[macro_use]
extern crate lazy_static;
#[cfg(test)]
extern crate many_files_dirs;
#[macro_use]
extern crate maplit;
extern crate mercurial_types;
extern crate metaconfig;
extern crate mononoke_types;
#[cfg(test)]
extern crate tempdir;

pub mod lua_hook;
pub mod rust_hook;
pub mod hook_loader;
pub mod errors;

use asyncmemo::{Asyncmemo, Filler, Weight};
use blobrepo::{BlobRepo, HgBlobChangeset};
use bookmarks::Bookmark;
use bytes::Bytes;
pub use errors::*;
use failure::Error;
use futures::{failed, finished, Future};
use futures_ext::{BoxFuture, FutureExt};
use mercurial_types::{Changeset, HgChangesetId, HgParents, MPath};
use mononoke_types::FileContents;
use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;
use std::mem;
use std::str;
use std::sync::{Arc, Mutex};

type ChangesetHooks = HashMap<String, Arc<Hook<HookChangeset>>>;
type FileHooks = Arc<Mutex<HashMap<String, Arc<Hook<HookFile>>>>>;
type Cache = Asyncmemo<HookCacheFiller>;

/// Manages hooks and allows them to be installed and uninstalled given a name
/// Knows how to run hooks
pub struct HookManager {
    cache: Cache,
    changeset_hooks: ChangesetHooks,
    file_hooks: FileHooks,
    bookmark_hooks: HashMap<Bookmark, Vec<String>>,
    repo_name: String,
    changeset_store: Box<ChangesetStore>,
}

impl HookManager {
    pub fn new(
        repo_name: String,
        changeset_store: Box<ChangesetStore>,
        content_store: Arc<FileContentStore>,
        entrylimit: usize,
        weightlimit: usize,
    ) -> HookManager {
        let changeset_hooks = HashMap::new();
        let file_hooks = Arc::new(Mutex::new(HashMap::new()));

        let filler = HookCacheFiller {
            file_hooks: file_hooks.clone(),
            repo_name: repo_name.clone(),
            content_store,
        };
        let cache = Asyncmemo::with_limits("hooks", filler, entrylimit, weightlimit);

        HookManager {
            cache,
            changeset_hooks,
            file_hooks,
            bookmark_hooks: HashMap::new(),
            repo_name,
            changeset_store,
        }
    }

    pub fn register_changeset_hook(&mut self, hook_name: &str, hook: Arc<Hook<HookChangeset>>) {
        self.changeset_hooks.insert(hook_name.to_string(), hook);
    }

    pub fn register_file_hook(&mut self, hook_name: &str, hook: Arc<Hook<HookFile>>) {
        let mut hooks = self.file_hooks.lock().unwrap();
        hooks.insert(hook_name.to_string(), hook);
    }

    pub fn set_hooks_for_bookmark(&mut self, bookmark: Bookmark, hooks: Vec<String>) {
        self.bookmark_hooks.insert(bookmark, hooks);
    }

    pub fn changeset_hook_names(&self) -> HashSet<String> {
        self.changeset_hooks
            .iter()
            .map(|(name, _)| name.clone())
            .collect()
    }

    pub fn file_hook_names(&self) -> HashSet<String> {
        self.file_hooks
            .lock()
            .unwrap()
            .iter()
            .map(|(name, _)| name.clone())
            .collect()
    }

    // Changeset hooks

    pub fn run_changeset_hooks_for_bookmark(
        &self,
        changeset_id: HgChangesetId,
        bookmark: &Bookmark,
    ) -> BoxFuture<Vec<(String, HookExecution)>, Error> {
        match self.bookmark_hooks.get(bookmark) {
            Some(hooks) => self.run_changeset_hooks_for_changeset_id(changeset_id, hooks.to_vec()),
            None => return finished(Vec::new()).boxify(),
        }
    }

    fn run_changeset_hooks_for_changeset_id(
        &self,
        changeset_id: HgChangesetId,
        hooks: Vec<String>,
    ) -> BoxFuture<Vec<(String, HookExecution)>, Error> {
        let hooks: Result<Vec<(String, Arc<Hook<HookChangeset>>)>, Error> = hooks
            .iter()
            .map(|hook_name| {
                let hook = self.changeset_hooks
                    .get(hook_name)
                    .ok_or(ErrorKind::NoSuchHook(hook_name.to_string()))?;
                Ok((hook_name.clone(), hook.clone()))
            })
            .collect();
        let hooks = try_boxfuture!(hooks);
        let repo_name = self.repo_name.clone();
        self.get_hook_changeset(changeset_id)
            .and_then(move |hcs| {
                HookManager::run_changeset_hooks_for_changeset(
                    repo_name,
                    hcs.clone(),
                    hooks.clone(),
                )
            })
            .boxify()
    }

    fn run_changeset_hooks_for_changeset(
        repo_name: String,
        changeset: HookChangeset,
        hooks: Vec<(String, Arc<Hook<HookChangeset>>)>,
    ) -> BoxFuture<Vec<(String, HookExecution)>, Error> {
        let v: Vec<BoxFuture<(String, HookExecution), _>> = hooks
            .iter()
            .map(move |(hook_name, hook)| {
                let hook_context: HookContext<HookChangeset> =
                    HookContext::new(hook_name.clone(), repo_name.clone(), changeset.clone());
                HookManager::run_changeset_hook(hook.clone(), hook_context)
            })
            .collect();
        futures::future::join_all(v).boxify()
    }

    fn run_changeset_hook(
        hook: Arc<Hook<HookChangeset>>,
        hook_context: HookContext<HookChangeset>,
    ) -> BoxFuture<(String, HookExecution), Error> {
        let hook_name = hook_context.hook_name.clone();
        hook.run(hook_context)
            .map(move |he| (hook_name, he))
            .boxify()
    }

    // File hooks

    pub fn run_file_hooks_for_bookmark(
        &self,
        changeset_id: HgChangesetId,
        bookmark: &Bookmark,
    ) -> BoxFuture<Vec<(FileHookExecutionID, HookExecution)>, Error> {
        match self.bookmark_hooks.get(bookmark) {
            Some(hooks) => self.run_file_hooks_for_changeset_id(changeset_id, hooks.to_vec()),
            None => return Box::new(finished(Vec::new())),
        }
    }

    fn run_file_hooks_for_changeset_id(
        &self,
        changeset_id: HgChangesetId,
        hooks: Vec<String>,
    ) -> BoxFuture<Vec<(FileHookExecutionID, HookExecution)>, Error> {
        let cache = self.cache.clone();
        self.get_hook_changeset(changeset_id)
            .and_then(move |hcs| {
                HookManager::run_file_hooks_for_changeset(
                    changeset_id,
                    hcs.clone(),
                    hooks.clone(),
                    cache,
                )
            })
            .boxify()
    }

    fn run_file_hooks_for_changeset(
        changeset_id: HgChangesetId,
        changeset: HookChangeset,
        hooks: Vec<String>,
        cache: Cache,
    ) -> BoxFuture<Vec<(FileHookExecutionID, HookExecution)>, Error> {
        let v: Vec<BoxFuture<Vec<(FileHookExecutionID, HookExecution)>, _>> = changeset
            .files
            .iter()
            .map(move |path| {
                HookManager::run_file_hooks(
                    changeset_id,
                    path.to_string(),
                    hooks.clone(),
                    cache.clone(),
                )
            })
            .collect();
        futures::future::join_all(v)
            .map(|vv| vv.into_iter().flatten().collect())
            .boxify()
    }

    fn run_file_hooks(
        cs_id: HgChangesetId,
        path: String,
        hooks: Vec<String>,
        cache: Cache,
    ) -> BoxFuture<Vec<(FileHookExecutionID, HookExecution)>, Error> {
        let v: Vec<BoxFuture<(FileHookExecutionID, HookExecution), _>> = hooks
            .iter()
            .map(move |hook_name| {
                HookManager::run_file_hook(
                    FileHookExecutionID {
                        cs_id,
                        hook_name: hook_name.to_string(),
                        path: path.clone(),
                    },
                    cache.clone(),
                )
            })
            .collect();
        futures::future::join_all(v).boxify()
    }

    fn run_file_hook(
        key: FileHookExecutionID,
        cache: Cache,
    ) -> BoxFuture<(FileHookExecutionID, HookExecution), Error> {
        cache.get(key.clone()).map(|he| (key, he)).boxify()
    }

    fn get_hook_changeset(&self, changeset_id: HgChangesetId) -> BoxFuture<HookChangeset, Error> {
        Box::new(
            self.changeset_store
                .get_changeset_by_changesetid(&changeset_id)
                .then(|res| match res {
                    Ok(cs) => HookChangeset::try_from(cs),
                    Err(e) => Err(e),
                }),
        )
    }
}

pub trait Hook<T>: Send + Sync
where
    T: Clone,
{
    fn run(&self, hook_context: HookContext<T>) -> BoxFuture<HookExecution, Error>;
}

/// Represents a changeset - more user friendly than the blob changeset
/// as this uses String not Vec[u8]
#[derive(Clone, Debug, PartialEq)]
pub struct HookChangeset {
    pub author: String,
    pub files: Vec<String>,
    pub comments: String,
    pub parents: HookChangesetParents,
}

#[derive(Clone)]
pub struct HookFile {
    pub path: String,
    content_store: Arc<FileContentStore>,
    changeset_id: HgChangesetId,
}

impl HookFile {
    pub fn new(
        path: String,
        content_store: Arc<FileContentStore>,
        changeset_id: HgChangesetId,
    ) -> HookFile {
        HookFile {
            path,
            content_store,
            changeset_id,
        }
    }

    pub fn contains_string(&self, data: &str) -> BoxFuture<bool, Error> {
        let path = try_boxfuture!(MPath::new(self.path.as_bytes()));
        let data = data.to_string();
        self.content_store
            .get_file_content_for_changeset(self.changeset_id, path)
            .and_then(move |bytes| {
                let str_content = str::from_utf8(&bytes)?.to_string();
                Ok(str_content.contains(&data))
            })
            .boxify()
    }
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

pub trait ChangesetStore: Send + Sync {
    fn get_changeset_by_changesetid(
        &self,
        changesetid: &HgChangesetId,
    ) -> BoxFuture<HgBlobChangeset, Error>;
}

pub struct BlobRepoChangesetStore {
    pub repo: BlobRepo,
}

impl ChangesetStore for BlobRepoChangesetStore {
    fn get_changeset_by_changesetid(
        &self,
        changesetid: &HgChangesetId,
    ) -> BoxFuture<HgBlobChangeset, Error> {
        self.repo.get_changeset_by_changesetid(changesetid)
    }
}

impl BlobRepoChangesetStore {
    pub fn new(repo: BlobRepo) -> BlobRepoChangesetStore {
        BlobRepoChangesetStore { repo }
    }
}

pub struct InMemoryChangesetStore {
    map: HashMap<HgChangesetId, HgBlobChangeset>,
}

impl ChangesetStore for InMemoryChangesetStore {
    fn get_changeset_by_changesetid(
        &self,
        changesetid: &HgChangesetId,
    ) -> BoxFuture<HgBlobChangeset, Error> {
        match self.map.get(changesetid) {
            Some(cs) => Box::new(finished(cs.clone())),
            None => Box::new(failed(
                ErrorKind::NoSuchChangeset(changesetid.to_string()).into(),
            )),
        }
    }
}

impl InMemoryChangesetStore {
    pub fn new() -> InMemoryChangesetStore {
        InMemoryChangesetStore {
            map: HashMap::new(),
        }
    }

    pub fn insert(&mut self, changeset_id: &HgChangesetId, changeset: &HgBlobChangeset) {
        self.map.insert(changeset_id.clone(), changeset.clone());
    }
}

pub trait FileContentStore: Send + Sync {
    fn get_file_content_for_changeset(
        &self,
        changesetid: HgChangesetId,
        path: MPath,
    ) -> BoxFuture<Bytes, Error>;
}

pub struct InMemoryFileContentStore {
    map: HashMap<(HgChangesetId, MPath), Bytes>,
}

impl FileContentStore for InMemoryFileContentStore {
    fn get_file_content_for_changeset(
        &self,
        changesetid: HgChangesetId,
        path: MPath,
    ) -> BoxFuture<Bytes, Error> {
        let fut = match self.map.get(&(changesetid, path.clone())) {
            Some(bytes) => finished(bytes.clone()),
            None => failed(ErrorKind::NoFileContent(changesetid, path.into()).into()),
        };
        fut.boxify()
    }
}

impl InMemoryFileContentStore {
    pub fn new() -> InMemoryFileContentStore {
        InMemoryFileContentStore {
            map: HashMap::new(),
        }
    }

    pub fn insert(&mut self, key: (HgChangesetId, MPath), content: Bytes) {
        self.map.insert(key, content);
    }
}

// TODO this can cache file content locally to prevent unnecessary lookup of changeset,
// manifest and walk of manifest each time
// It's likely that multiple hooks will want to bsee the same content for the same changeset
pub struct BlobRepoFileContentStore {
    pub repo: BlobRepo,
}

impl FileContentStore for BlobRepoFileContentStore {
    fn get_file_content_for_changeset(
        &self,
        changesetid: HgChangesetId,
        path: MPath,
    ) -> BoxFuture<Bytes, Error> {
        let repo = self.repo.clone();
        let repo2 = repo.clone();
        let path2 = path.clone();
        repo.get_changeset_by_changesetid(&changesetid)
            .and_then(move |changeset| {
                repo.find_file_in_manifest(&path, changeset.manifestid().clone())
            })
            .and_then(move |opt| {
                opt.ok_or(ErrorKind::NoFileContent(changesetid, path2.into()).into())
            })
            .and_then(move |hash| repo2.get_file_content(&hash.into_nodehash()))
            .and_then(|content| {
                let FileContents::Bytes(bytes) = content;
                Ok(bytes)
            })
            .boxify()
    }
}

impl BlobRepoFileContentStore {
    pub fn new(repo: BlobRepo) -> BlobRepoFileContentStore {
        BlobRepoFileContentStore { repo }
    }
}

struct HookCacheFiller {
    repo_name: String,
    file_hooks: FileHooks,
    content_store: Arc<FileContentStore>,
}

impl Filler for HookCacheFiller {
    type Key = FileHookExecutionID;
    type Value = BoxFuture<HookExecution, Error>;

    fn fill(&self, _cache: &Asyncmemo<Self>, key: &Self::Key) -> Self::Value {
        let hooks = self.file_hooks.lock().unwrap();
        match hooks.get(&key.hook_name) {
            Some(arc_hook) => {
                let arc_hook = arc_hook.clone();
                let hook_file = HookFile::new(
                    key.path.clone(),
                    self.content_store.clone(),
                    key.cs_id.clone(),
                );
                let hook_context: HookContext<HookFile> =
                    HookContext::new(key.hook_name.clone(), self.repo_name.clone(), hook_file);
                arc_hook.run(hook_context)
            }
            None => panic!("Can't find hook"), // TODO
        }
    }
}

#[derive(Clone, Debug, PartialEq, Hash, Eq)]
// TODO Note that when we move to Bonsai changesets the ID that we use in the cache will
// be the content hash
pub struct FileHookExecutionID {
    pub cs_id: HgChangesetId,
    pub hook_name: String,
    pub path: String,
}

impl Weight for FileHookExecutionID {
    fn get_weight(&self) -> usize {
        self.cs_id.get_weight() + self.hook_name.get_weight() + self.path.get_weight()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum HookChangesetParents {
    None,
    One(String),
    Two(String, String),
}

impl TryFrom<HgBlobChangeset> for HookChangeset {
    type Error = Error;
    fn try_from(changeset: HgBlobChangeset) -> Result<Self, Error> {
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

#[derive(Clone, Debug, PartialEq)]
pub struct HookContext<T>
where
    T: Clone,
{
    pub hook_name: String,
    pub repo_name: String,
    pub data: T,
}

impl<T> HookContext<T>
where
    T: Clone,
{
    fn new(hook_name: String, repo_name: String, data: T) -> HookContext<T> {
        HookContext {
            hook_name,
            repo_name,
            data,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use futures::Future;
    use futures::future::finished;
    use many_files_dirs;
    use std::collections::hash_map::Entry;
    use std::str::FromStr;

    #[derive(Clone, Debug)]
    struct FnChangesetHook {
        f: fn(HookContext<HookChangeset>) -> HookExecution,
    }

    impl FnChangesetHook {
        fn new(f: fn(HookContext<HookChangeset>) -> HookExecution) -> FnChangesetHook {
            FnChangesetHook { f }
        }
    }

    impl Hook<HookChangeset> for FnChangesetHook {
        fn run(&self, context: HookContext<HookChangeset>) -> BoxFuture<HookExecution, Error> {
            finished((self.f)(context)).boxify()
        }
    }

    fn always_accepting_changeset_hook() -> Box<Hook<HookChangeset>> {
        let f: fn(HookContext<HookChangeset>) -> HookExecution = |_| HookExecution::Accepted;
        Box::new(FnChangesetHook::new(f))
    }

    fn always_rejecting_changeset_hook() -> Box<Hook<HookChangeset>> {
        let f: fn(HookContext<HookChangeset>) -> HookExecution = |_| default_rejection();
        Box::new(FnChangesetHook::new(f))
    }

    #[derive(Clone, Debug)]
    struct ContextMatchingChangesetHook {
        expected_context: HookContext<HookChangeset>,
    }

    impl Hook<HookChangeset> for ContextMatchingChangesetHook {
        fn run(&self, context: HookContext<HookChangeset>) -> BoxFuture<HookExecution, Error> {
            assert_eq!(self.expected_context, context);
            Box::new(finished(HookExecution::Accepted))
        }
    }

    fn context_matching_changeset_hook(
        expected_context: HookContext<HookChangeset>,
    ) -> Box<Hook<HookChangeset>> {
        Box::new(ContextMatchingChangesetHook { expected_context })
    }

    #[derive(Clone, Debug)]
    struct FnFileHook {
        f: fn(HookContext<HookFile>) -> HookExecution,
    }

    impl FnFileHook {
        fn new(f: fn(HookContext<HookFile>) -> HookExecution) -> FnFileHook {
            FnFileHook { f }
        }
    }

    impl Hook<HookFile> for FnFileHook {
        fn run(&self, context: HookContext<HookFile>) -> BoxFuture<HookExecution, Error> {
            finished((self.f)(context)).boxify()
        }
    }

    fn always_accepting_file_hook() -> Box<Hook<HookFile>> {
        let f: fn(HookContext<HookFile>) -> HookExecution = |_| HookExecution::Accepted;
        Box::new(FnFileHook::new(f))
    }

    fn always_rejecting_file_hook() -> Box<Hook<HookFile>> {
        let f: fn(HookContext<HookFile>) -> HookExecution = |_| default_rejection();
        Box::new(FnFileHook::new(f))
    }

    #[derive(Clone, Debug)]
    struct PathMatchingFileHook {
        paths: HashSet<String>,
    }

    impl Hook<HookFile> for PathMatchingFileHook {
        fn run(&self, context: HookContext<HookFile>) -> BoxFuture<HookExecution, Error> {
            finished(if self.paths.contains(&context.data.path) {
                HookExecution::Accepted
            } else {
                default_rejection()
            }).boxify()
        }
    }

    fn path_matching_file_hook(paths: HashSet<String>) -> Box<Hook<HookFile>> {
        Box::new(PathMatchingFileHook { paths })
    }

    #[derive(Clone, Debug)]
    struct ContentMatchingFileHook {
        content: String,
    }

    impl Hook<HookFile> for ContentMatchingFileHook {
        fn run(&self, context: HookContext<HookFile>) -> BoxFuture<HookExecution, Error> {
            context
                .data
                .contains_string(&self.content)
                .map(|contains| {
                    if contains {
                        HookExecution::Accepted
                    } else {
                        default_rejection()
                    }
                })
                .boxify()
        }
    }

    fn content_matching_file_hook(content: String) -> Box<Hook<HookFile>> {
        Box::new(ContentMatchingFileHook { content })
    }

    #[test]
    fn test_changeset_hook_accepted() {
        async_unit::tokio_unit_test(|| {
            let hooks: HashMap<String, Box<Hook<HookChangeset>>> = hashmap! {
                "hook1".to_string() => always_accepting_changeset_hook()
            };
            let bookmarks = hashmap! {
                "bm1".to_string() => vec!["hook1".to_string()]
            };
            let expected = hashmap! {
                "hook1".to_string() => HookExecution::Accepted
            };
            run_changeset_hooks("bm1", hooks, bookmarks, expected);
        });
    }

    #[test]
    fn test_changeset_hook_rejected() {
        async_unit::tokio_unit_test(|| {
            let hooks: HashMap<String, Box<Hook<HookChangeset>>> = hashmap! {
                "hook1".to_string() => always_rejecting_changeset_hook()
            };
            let bookmarks = hashmap! {
                "bm1".to_string() => vec!["hook1".to_string()]
            };
            let expected = hashmap! {
                "hook1".to_string() => default_rejection()
            };
            run_changeset_hooks("bm1", hooks, bookmarks, expected);
        });
    }

    #[test]
    fn test_changeset_hook_mix() {
        async_unit::tokio_unit_test(|| {
            let hooks: HashMap<String, Box<Hook<HookChangeset>>> = hashmap! {
                "hook1".to_string() => always_accepting_changeset_hook(),
                "hook2".to_string() => always_rejecting_changeset_hook(),
                "hook3".to_string() => always_accepting_changeset_hook(),
            };
            let bookmarks = hashmap! {
                "bm1".to_string() => vec!["hook1".to_string(), "hook2".to_string(),
                 "hook3".to_string()]
            };
            let expected = hashmap! {
                "hook1".to_string() => HookExecution::Accepted,
                "hook2".to_string() => default_rejection(),
                "hook3".to_string() => HookExecution::Accepted,
            };
            run_changeset_hooks("bm1", hooks, bookmarks, expected);
        });
    }

    #[test]
    fn test_changeset_hook_context() {
        async_unit::tokio_unit_test(|| {
            let files = vec![
                "dir1/subdir1/subsubdir1/file_1".into(),
                "dir1/subdir1/subsubdir2/file_1".into(),
                "dir1/subdir1/subsubdir2/file_2".into(),
            ];
            let parents =
                HookChangesetParents::One("ecafdc4a4b6748b7a7215c6995f14c837dc1ebec".into());
            let data = HookChangeset::new(
                "Stanislau Hlebik <stash@fb.com>".into(),
                files,
                "Stanislau Hlebik <stash@fb.com>".into(),
                parents,
            );
            let expected_context = HookContext {
                hook_name: "hook1".into(),
                repo_name: "some_repo".into(),
                data,
            };
            let hooks: HashMap<String, Box<Hook<HookChangeset>>> = hashmap! {
                "hook1".to_string() => context_matching_changeset_hook(expected_context)
            };
            let bookmarks = hashmap! {
                "bm1".to_string() => vec!["hook1".to_string()]
            };
            let expected = hashmap! {
                "hook1".to_string() => HookExecution::Accepted
            };
            run_changeset_hooks("bm1", hooks, bookmarks, expected);
        });
    }

    #[test]
    fn test_file_hook_accepted() {
        async_unit::tokio_unit_test(|| {
            let hooks: HashMap<String, Box<Hook<HookFile>>> = hashmap! {
                "hook1".to_string() => always_accepting_file_hook()
            };
            let bookmarks = hashmap! {
                "bm1".to_string() => vec!["hook1".to_string()]
            };
            let expected = hashmap! {
                "hook1".to_string() => hashmap! {
                    "dir1/subdir1/subsubdir1/file_1".to_string() => HookExecution::Accepted,
                    "dir1/subdir1/subsubdir2/file_1".to_string() => HookExecution::Accepted,
                    "dir1/subdir1/subsubdir2/file_2".to_string() => HookExecution::Accepted,
                }
            };
            run_file_hooks("bm1", hooks, bookmarks, expected);
        });
    }

    #[test]
    fn test_file_hook_rejected() {
        async_unit::tokio_unit_test(move || {
            let hooks: HashMap<String, Box<Hook<HookFile>>> = hashmap! {
                "hook1".to_string() => always_rejecting_file_hook()
            };
            let bookmarks = hashmap! {
                "bm1".to_string() => vec!["hook1".to_string()]
            };
            let expected = hashmap! {
                "hook1".to_string() => hashmap! {
                    "dir1/subdir1/subsubdir1/file_1".to_string() => default_rejection(),
                    "dir1/subdir1/subsubdir2/file_1".to_string() => default_rejection(),
                    "dir1/subdir1/subsubdir2/file_2".to_string() => default_rejection(),
                }
            };
            run_file_hooks("bm1", hooks, bookmarks, expected);
        });
    }

    #[test]
    fn test_file_hook_mix() {
        async_unit::tokio_unit_test(move || {
            let hooks: HashMap<String, Box<Hook<HookFile>>> = hashmap! {
                "hook1".to_string() => always_rejecting_file_hook(),
                "hook2".to_string() => always_accepting_file_hook()
            };
            let bookmarks = hashmap! {
                "bm1".to_string() => vec!["hook1".to_string(), "hook2".to_string()]
            };
            let expected = hashmap! {
                "hook1".to_string() => hashmap! {
                    "dir1/subdir1/subsubdir1/file_1".to_string() => default_rejection(),
                    "dir1/subdir1/subsubdir2/file_1".to_string() => default_rejection(),
                    "dir1/subdir1/subsubdir2/file_2".to_string() => default_rejection(),
                },
                "hook2".to_string() => hashmap! {
                    "dir1/subdir1/subsubdir1/file_1".to_string() => HookExecution::Accepted,
                    "dir1/subdir1/subsubdir2/file_1".to_string() => HookExecution::Accepted,
                    "dir1/subdir1/subsubdir2/file_2".to_string() => HookExecution::Accepted,
                }
            };
            run_file_hooks("bm1", hooks, bookmarks, expected);
        });
    }

    #[test]
    fn test_file_hooks_paths() {
        async_unit::tokio_unit_test(move || {
            let matching_paths = hashset![
                "dir1/subdir1/subsubdir2/file_1".to_string(),
                "dir1/subdir1/subsubdir2/file_2".to_string(),
            ];
            let hooks: HashMap<String, Box<Hook<HookFile>>> = hashmap! {
                "hook1".to_string() => path_matching_file_hook(matching_paths),
            };
            let bookmarks = hashmap! {
                "bm1".to_string() => vec!["hook1".to_string()]
            };
            let expected = hashmap! {
                "hook1".to_string() => hashmap! {
                    "dir1/subdir1/subsubdir1/file_1".to_string() => default_rejection(),
                    "dir1/subdir1/subsubdir2/file_1".to_string() => HookExecution::Accepted,
                    "dir1/subdir1/subsubdir2/file_2".to_string() => HookExecution::Accepted,
                }
            };
            run_file_hooks("bm1", hooks, bookmarks, expected);
        });
    }

    #[test]
    fn test_file_hooks_paths_mix() {
        async_unit::tokio_unit_test(move || {
            let matching_paths1 = hashset![
                "dir1/subdir1/subsubdir2/file_1".to_string(),
                "dir1/subdir1/subsubdir2/file_2".to_string(),
            ];
            let matching_paths2 = hashset!["dir1/subdir1/subsubdir1/file_1".to_string(),];
            let hooks: HashMap<String, Box<Hook<HookFile>>> = hashmap! {
                "hook1".to_string() => path_matching_file_hook(matching_paths1),
                "hook2".to_string() => path_matching_file_hook(matching_paths2),
            };
            let bookmarks = hashmap! {
                "bm1".to_string() => vec!["hook1".to_string(), "hook2".to_string()]
            };
            let expected = hashmap! {
                "hook1".to_string() => hashmap! {
                    "dir1/subdir1/subsubdir1/file_1".to_string() => default_rejection(),
                    "dir1/subdir1/subsubdir2/file_1".to_string() => HookExecution::Accepted,
                    "dir1/subdir1/subsubdir2/file_2".to_string() => HookExecution::Accepted,
                },
                "hook2".to_string() => hashmap! {
                    "dir1/subdir1/subsubdir1/file_1".to_string() => HookExecution::Accepted,
                    "dir1/subdir1/subsubdir2/file_1".to_string() => default_rejection(),
                    "dir1/subdir1/subsubdir2/file_2".to_string() => default_rejection(),
                }
            };
            run_file_hooks("bm1", hooks, bookmarks, expected);
        });
    }

    #[test]
    fn test_file_hook_content() {
        async_unit::tokio_unit_test(|| {
            let hooks: HashMap<String, Box<Hook<HookFile>>> = hashmap! {
                "hook1".to_string() => content_matching_file_hook("content_d1_f1".to_string()),
                "hook2".to_string() => content_matching_file_hook("content_d2_f1".to_string()),
                "hook3".to_string() => content_matching_file_hook("content_d2_f2".to_string())
            };
            let bookmarks = hashmap! {
                "bm1".to_string() => vec!["hook1".to_string(),
                "hook2".to_string(), "hook3".to_string()]
            };
            let expected = hashmap! {
                "hook1".to_string() => hashmap! {
                    "dir1/subdir1/subsubdir1/file_1".to_string() => HookExecution::Accepted,
                    "dir1/subdir1/subsubdir2/file_1".to_string() => default_rejection(),
                    "dir1/subdir1/subsubdir2/file_2".to_string() => default_rejection(),
                },
                "hook2".to_string() => hashmap! {
                    "dir1/subdir1/subsubdir1/file_1".to_string() => default_rejection(),
                    "dir1/subdir1/subsubdir2/file_1".to_string() => HookExecution::Accepted,
                    "dir1/subdir1/subsubdir2/file_2".to_string() => default_rejection(),
                },
                "hook3".to_string() => hashmap! {
                    "dir1/subdir1/subsubdir1/file_1".to_string() => default_rejection(),
                    "dir1/subdir1/subsubdir2/file_1".to_string() => default_rejection(),
                    "dir1/subdir1/subsubdir2/file_2".to_string() => HookExecution::Accepted,
                },
            };
            run_file_hooks("bm1", hooks, bookmarks, expected);
        });
    }

    #[test]
    fn test_register_changeset_hooks() {
        async_unit::tokio_unit_test(|| {
            let mut hook_manager = hook_manager_inmem();
            let hook1 = always_accepting_changeset_hook();
            hook_manager.register_changeset_hook("hook1", hook1.into());
            let hook2 = always_accepting_changeset_hook();
            hook_manager.register_changeset_hook("hook2", hook2.into());

            let set = hook_manager.changeset_hook_names();
            assert_eq!(2, set.len());
            assert!(set.contains("hook1"));
            assert!(set.contains("hook1"));
        });
    }

    #[test]
    fn test_with_blob_store() {
        async_unit::tokio_unit_test(|| {
            let hooks: HashMap<String, Box<Hook<HookChangeset>>> = hashmap! {
                "hook1".to_string() => always_accepting_changeset_hook()
            };
            let bookmarks = hashmap! {
                "bm1".to_string() => vec!["hook1".to_string()]
            };
            let expected = hashmap! {
                "hook1".to_string() => HookExecution::Accepted
            };
            run_changeset_hooks_with_mgr("bm1", hooks, bookmarks, expected, false);
        });
    }

    fn run_changeset_hooks(
        bookmark_name: &str,
        hooks: HashMap<String, Box<Hook<HookChangeset>>>,
        bookmarks: HashMap<String, Vec<String>>,
        expected: HashMap<String, HookExecution>,
    ) {
        run_changeset_hooks_with_mgr(bookmark_name, hooks, bookmarks, expected, true)
    }

    fn run_changeset_hooks_with_mgr(
        bookmark_name: &str,
        hooks: HashMap<String, Box<Hook<HookChangeset>>>,
        bookmarks: HashMap<String, Vec<String>>,
        expected: HashMap<String, HookExecution>,
        inmem: bool,
    ) {
        let mut hook_manager = setup_hook_manager(bookmarks, inmem);
        for (hook_name, hook) in hooks {
            hook_manager.register_changeset_hook(&hook_name, hook.into());
        }
        let fut = hook_manager.run_changeset_hooks_for_bookmark(
            default_changeset_id(),
            &Bookmark::new(bookmark_name).unwrap(),
        );
        let res = fut.wait().unwrap();
        let map: HashMap<String, HookExecution> = res.into_iter().collect();
        assert_eq!(expected, map);
    }

    fn run_file_hooks(
        bookmark_name: &str,
        hooks: HashMap<String, Box<Hook<HookFile>>>,
        bookmarks: HashMap<String, Vec<String>>,
        expected: HashMap<String, HashMap<String, HookExecution>>,
    ) {
        run_file_hooks_with_mgr(bookmark_name, hooks, bookmarks, expected, true)
    }

    fn run_file_hooks_with_mgr(
        bookmark_name: &str,
        hooks: HashMap<String, Box<Hook<HookFile>>>,
        bookmarks: HashMap<String, Vec<String>>,
        expected: HashMap<String, HashMap<String, HookExecution>>,
        inmem: bool,
    ) {
        let mut hook_manager = setup_hook_manager(bookmarks, inmem);
        for (hook_name, hook) in hooks {
            hook_manager.register_file_hook(&hook_name, hook.into());
        }
        let fut: BoxFuture<Vec<(FileHookExecutionID, HookExecution)>, Error> = hook_manager
            .run_file_hooks_for_bookmark(
                default_changeset_id(),
                &Bookmark::new(bookmark_name).unwrap(),
            );
        let res = fut.wait().unwrap();
        let map: HashMap<String, HashMap<String, HookExecution>> =
            res.into_iter()
                .fold(HashMap::new(), |mut m, (exec_id, exec)| {
                    match m.entry(exec_id.hook_name) {
                        Entry::Vacant(v) => v.insert(HashMap::new()).insert(exec_id.path, exec),
                        Entry::Occupied(mut v) => v.get_mut().insert(exec_id.path, exec),
                    };
                    m
                });
        assert_eq!(expected, map);
    }

    fn setup_hook_manager(bookmarks: HashMap<String, Vec<String>>, inmem: bool) -> HookManager {
        let mut hook_manager = if inmem {
            hook_manager_inmem()
        } else {
            hook_manager_blobrepo()
        };
        for (bookmark_name, hook_names) in bookmarks {
            hook_manager.set_hooks_for_bookmark(Bookmark::new(bookmark_name).unwrap(), hook_names);
        }
        hook_manager
    }

    fn default_rejection() -> HookExecution {
        HookExecution::Rejected(HookRejectionInfo::new("desc".into(), "long_desc".into()))
    }

    fn default_changeset_id() -> HgChangesetId {
        HgChangesetId::from_str("473b2e715e0df6b2316010908879a3c78e275dd9").unwrap()
    }

    fn hook_manager_blobrepo() -> HookManager {
        let repo = many_files_dirs::getrepo(None);
        let changeset_store = BlobRepoChangesetStore::new(repo.clone());
        let content_store = BlobRepoFileContentStore::new(repo);
        HookManager::new(
            "some_repo".into(),
            Box::new(changeset_store),
            Arc::new(content_store),
            1024,
            1024 * 1024,
        )
    }

    fn hook_manager_inmem() -> HookManager {
        let repo = many_files_dirs::getrepo(None);
        // Load up an in memory store with a single commit from the many_files_dirs store
        let cs_id = HgChangesetId::from_str("473b2e715e0df6b2316010908879a3c78e275dd9").unwrap();
        let cs = repo.get_changeset_by_changesetid(&cs_id).wait().unwrap();
        let mut changeset_store = InMemoryChangesetStore::new();
        changeset_store.insert(&cs_id, &cs);
        let mut content_store = InMemoryFileContentStore::new();
        content_store.insert(
            (cs_id.clone(), to_mpath("dir1/subdir1/subsubdir1/file_1")),
            "content_d1_f1".into(),
        );
        content_store.insert(
            (cs_id.clone(), to_mpath("dir1/subdir1/subsubdir2/file_1")),
            "content_d2_f1".into(),
        );
        content_store.insert(
            (cs_id.clone(), to_mpath("dir1/subdir1/subsubdir2/file_2")),
            "content_d2_f2".into(),
        );
        HookManager::new(
            "some_repo".into(),
            Box::new(changeset_store),
            Arc::new(content_store),
            1024,
            1024 * 1024,
        )
    }

    pub fn to_mpath(string: &str) -> MPath {
        // Please... avert your eyes
        MPath::new(string.to_string().as_bytes().to_vec()).unwrap()
    }

}
