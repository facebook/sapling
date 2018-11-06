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
extern crate cloned;
#[macro_use]
extern crate failure_ext as failure;
#[cfg(test)]
extern crate fixtures;
extern crate futures;
#[macro_use]
extern crate futures_ext;
extern crate hlua;
extern crate hlua_futures;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate maplit;
extern crate mercurial_types;
extern crate metaconfig;
extern crate mononoke_types;
#[cfg(test)]
#[macro_use]
extern crate pretty_assertions;
extern crate regex;
#[macro_use]
extern crate slog;
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
use failure::{Error, FutureFailureErrorExt};
use futures::{failed, finished, Future, IntoFuture, Stream};
use futures_ext::{BoxFuture, FutureExt};
use mercurial_types::{Changeset, HgChangesetId, HgParents, MPath, manifest::get_empty_manifest,
                      manifest_utils::{self, EntryStatus}};
use metaconfig::repoconfig::HookBypass;
use mononoke_types::{FileContents, FileType};
use slog::Logger;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::mem;
use std::str;
use std::sync::{Arc, Mutex};

type ChangesetHooks = HashMap<String, (Arc<Hook<HookChangeset>>, Option<HookBypass>)>;
type FileHooks = Arc<Mutex<HashMap<String, (Arc<Hook<HookFile>>, Option<HookBypass>)>>>;
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
    content_store: Arc<FileContentStore>,
    logger: Logger,
}

impl HookManager {
    pub fn new(
        repo_name: String,
        changeset_store: Box<ChangesetStore>,
        content_store: Arc<FileContentStore>,
        entrylimit: usize,
        weightlimit: usize,
        logger: Logger,
    ) -> HookManager {
        let changeset_hooks = HashMap::new();
        let file_hooks = Arc::new(Mutex::new(HashMap::new()));

        let filler = HookCacheFiller {
            file_hooks: file_hooks.clone(),
            repo_name: repo_name.clone(),
        };
        let cache = Asyncmemo::with_limits("hooks", filler, entrylimit, weightlimit);

        HookManager {
            cache,
            changeset_hooks,
            file_hooks,
            bookmark_hooks: HashMap::new(),
            repo_name,
            changeset_store,
            content_store,
            logger,
        }
    }

    pub fn new_with_blobrepo(blobrepo: BlobRepo, logger: Logger) -> HookManager {
        HookManager::new(
            format!("repo-{:?}", blobrepo.get_repoid()),
            Box::new(BlobRepoChangesetStore::new(blobrepo.clone())),
            Arc::new(BlobRepoFileContentStore::new(blobrepo.clone())),
            1024 * 1024, // TODO make configurable T34438181
            1024 * 1024 * 1024,
            logger,
        )
    }

    pub fn register_changeset_hook(
        &mut self,
        hook_name: &str,
        hook: Arc<Hook<HookChangeset>>,
        bypass: Option<HookBypass>,
    ) {
        self.changeset_hooks
            .insert(hook_name.to_string(), (hook, bypass));
    }

    pub fn register_file_hook(
        &mut self,
        hook_name: &str,
        hook: Arc<Hook<HookFile>>,
        bypass: Option<HookBypass>,
    ) {
        let mut hooks = self.file_hooks.lock().unwrap();
        hooks.insert(hook_name.to_string(), (hook, bypass));
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
        maybe_pushvars: Option<HashMap<String, Bytes>>,
    ) -> BoxFuture<Vec<(ChangesetHookExecutionID, HookExecution)>, Error> {
        match self.bookmark_hooks.get(bookmark) {
            Some(hooks) => {
                let hooks = hooks
                    .clone()
                    .into_iter()
                    .filter(|name| self.changeset_hooks.contains_key(name))
                    .collect();
                self.run_changeset_hooks_for_changeset_id(changeset_id, hooks, maybe_pushvars)
            }
            None => return finished(Vec::new()).boxify(),
        }
    }

    fn run_changeset_hooks_for_changeset_id(
        &self,
        changeset_id: HgChangesetId,
        hooks: Vec<String>,
        maybe_pushvars: Option<HashMap<String, Bytes>>,
    ) -> BoxFuture<Vec<(ChangesetHookExecutionID, HookExecution)>, Error> {
        let hooks: Result<Vec<(String, (Arc<Hook<HookChangeset>>, _))>, Error> = hooks
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
            .and_then({
                move |hcs| {
                    let hooks = HookManager::filter_bypassed_hooks(
                        hooks,
                        &hcs.comments,
                        maybe_pushvars.as_ref(),
                    );

                    HookManager::run_changeset_hooks_for_changeset(
                        repo_name,
                        hcs.clone(),
                        hooks.clone(),
                    )
                }
            })
            .map(move |res| {
                res.into_iter()
                    .map(|(hook_name, exec)| {
                        (
                            ChangesetHookExecutionID {
                                cs_id: changeset_id,
                                hook_name,
                            },
                            exec,
                        )
                    })
                    .collect()
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
            .map({
                cloned!(hook_name);
                move |he| (hook_name, he)
            })
            .with_context(move |_| format!("while executing hook {}", hook_name))
            .from_err()
            .boxify()
    }

    // File hooks

    pub fn run_file_hooks_for_bookmark(
        &self,
        changeset_id: HgChangesetId,
        bookmark: &Bookmark,
        maybe_pushvars: Option<HashMap<String, Bytes>>,
    ) -> BoxFuture<Vec<(FileHookExecutionID, HookExecution)>, Error> {
        debug!(
            self.logger.clone(),
            "Running file hooks for bookmark {:?}",
            bookmark
        );
        match self.bookmark_hooks.get(bookmark) {
            Some(hooks) => {
                let file_hooks = self.file_hooks.lock().unwrap();
                let hooks = hooks
                    .clone()
                    .into_iter()
                    .filter_map(|name| file_hooks.get(&name).map(|hook| (name, hook.clone())))
                    .collect();
                self.run_file_hooks_for_changeset_id(
                    changeset_id,
                    hooks,
                    maybe_pushvars,
                    self.logger.clone(),
                )
            }
            None => return Box::new(finished(Vec::new())),
        }
    }

    fn run_file_hooks_for_changeset_id(
        &self,
        changeset_id: HgChangesetId,
        hooks: Vec<(String, (Arc<Hook<HookFile>>, Option<HookBypass>))>,
        maybe_pushvars: Option<HashMap<String, Bytes>>,
        logger: Logger,
    ) -> BoxFuture<Vec<(FileHookExecutionID, HookExecution)>, Error> {
        debug!(
            self.logger,
            "Running file hooks for changeset id {:?}", changeset_id
        );
        let cache = self.cache.clone();
        self.get_hook_changeset(changeset_id)
            .and_then(move |hcs| {
                let hooks = HookManager::filter_bypassed_hooks(
                    hooks.clone(),
                    &hcs.comments,
                    maybe_pushvars.as_ref(),
                );
                let hooks = hooks.into_iter().map(|(name, _)| name).collect();

                HookManager::run_file_hooks_for_changeset(
                    changeset_id,
                    hcs.clone(),
                    hooks,
                    cache,
                    logger,
                )
            })
            .boxify()
    }

    fn run_file_hooks_for_changeset(
        changeset_id: HgChangesetId,
        changeset: HookChangeset,
        hooks: Vec<String>,
        cache: Cache,
        logger: Logger,
    ) -> BoxFuture<Vec<(FileHookExecutionID, HookExecution)>, Error> {
        let v: Vec<BoxFuture<Vec<(FileHookExecutionID, HookExecution)>, _>> = changeset
            .files
            .iter()
            // Do not run file hooks for deleted files
            .filter_map(move |file| {
                match file.ty {
                    ChangedFileType::Added | ChangedFileType::Modified => Some(
                        HookManager::run_file_hooks(
                            changeset_id,
                            file.clone(),
                            hooks.clone(),
                            cache.clone(),
                            logger.clone(),
                        )
                    ),
                    ChangedFileType::Deleted => None,
                }
            })
            .collect();
        futures::future::join_all(v)
            .map(|vv| vv.into_iter().flatten().collect())
            .boxify()
    }

    fn run_file_hooks(
        cs_id: HgChangesetId,
        file: HookFile,
        hooks: Vec<String>,
        cache: Cache,
        logger: Logger,
    ) -> BoxFuture<Vec<(FileHookExecutionID, HookExecution)>, Error> {
        let v: Vec<BoxFuture<(FileHookExecutionID, HookExecution), _>> = hooks
            .iter()
            .map(move |hook_name| {
                HookManager::run_file_hook(
                    FileHookExecutionID {
                        cs_id,
                        hook_name: hook_name.to_string(),
                        file: file.clone(),
                    },
                    cache.clone(),
                    logger.clone(),
                )
            })
            .collect();
        futures::future::join_all(v).boxify()
    }

    fn run_file_hook(
        key: FileHookExecutionID,
        cache: Cache,
        logger: Logger,
    ) -> BoxFuture<(FileHookExecutionID, HookExecution), Error> {
        debug!(logger, "Running file hook {:?}", key);
        let hook_name = key.hook_name.clone();
        cache
            .get(key.clone())
            .map(|he| (key, he))
            .with_context(move |_| format!("while executing hook {}", hook_name))
            .from_err()
            .boxify()
    }

    fn get_hook_changeset(&self, changeset_id: HgChangesetId) -> BoxFuture<HookChangeset, Error> {
        let content_store = self.content_store.clone();
        let hg_changeset = self.changeset_store
            .get_changeset_by_changesetid(&changeset_id);
        let changed_files = self.changeset_store.get_changed_files(&changeset_id);
        Box::new((hg_changeset, changed_files).into_future().and_then(
            move |(changeset, changed_files)| {
                let author = str::from_utf8(changeset.user())?.into();
                let files = changed_files
                    .into_iter()
                    .map(|(path, ty)| {
                        HookFile::new(path, content_store.clone(), changeset_id.clone(), ty)
                    })
                    .collect();
                let comments = str::from_utf8(changeset.comments())?.into();
                let parents = HookChangesetParents::from(changeset.parents());
                Ok(HookChangeset::new(
                    author,
                    files,
                    comments,
                    parents,
                    changeset_id,
                    content_store,
                ))
            },
        ))
    }

    fn filter_bypassed_hooks<T: Clone>(
        hooks: Vec<(String, (T, Option<HookBypass>))>,
        commit_msg: &String,
        maybe_pushvars: Option<&HashMap<String, Bytes>>,
    ) -> Vec<(String, T)> {
        hooks
            .clone()
            .into_iter()
            .filter_map(|(hook_name, (hook, bypass))| match bypass {
                Some(bypass) => {
                    if HookManager::is_hook_bypassed(&bypass, commit_msg, maybe_pushvars) {
                        None
                    } else {
                        Some((hook_name, hook))
                    }
                }
                None => Some((hook_name, hook)),
            })
            .collect()
    }

    fn is_hook_bypassed(
        bypass: &HookBypass,
        cs_msg: &String,
        maybe_pushvars: Option<&HashMap<String, Bytes>>,
    ) -> bool {
        match bypass {
            HookBypass::CommitMessage(bypass_string) => cs_msg.contains(bypass_string),
            HookBypass::Pushvar { name, value } => {
                if let Some(pushvars) = maybe_pushvars {
                    let pushvar_val = pushvars
                        .get(name)
                        .map(|bytes| String::from_utf8(bytes.to_vec()));

                    if let Some(Ok(pushvar_val)) = pushvar_val {
                        return &pushvar_val == value;
                    }
                    return false;
                }
                return false;
            }
        }
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
#[derive(Clone)]
pub struct HookChangeset {
    pub author: String,
    pub files: Vec<HookFile>,
    pub comments: String,
    pub parents: HookChangesetParents,
    content_store: Arc<FileContentStore>,
    changeset_id: HgChangesetId,
}

impl fmt::Debug for HookChangeset {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "HookChangeset changeset_id: {:?} files: {:?}, comments: {:?}",
            self.changeset_id, self.files, self.comments
        )
    }
}

impl PartialEq for HookChangeset {
    fn eq(&self, other: &HookChangeset) -> bool {
        self.changeset_id == other.changeset_id
    }
}

#[derive(Clone)]
pub enum ChangedFileType {
    Added,
    Deleted,
    Modified,
}

impl From<EntryStatus> for ChangedFileType {
    fn from(entry_status: EntryStatus) -> Self {
        match entry_status {
            EntryStatus::Added(_) => ChangedFileType::Added,
            EntryStatus::Deleted(_) => ChangedFileType::Deleted,
            EntryStatus::Modified { .. } => ChangedFileType::Modified,
        }
    }
}

#[derive(Clone)]
pub struct HookFile {
    pub path: String,
    content_store: Arc<FileContentStore>,
    changeset_id: HgChangesetId,
    ty: ChangedFileType,
}

impl fmt::Debug for HookFile {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "HookFile path: {}, changeset_id: {}",
            self.path, self.changeset_id
        )
    }
}

impl PartialEq for HookFile {
    fn eq(&self, other: &HookFile) -> bool {
        self.path == other.path && self.changeset_id == other.changeset_id
    }
}

impl Weight for HookFile {
    fn get_weight(&self) -> usize {
        self.path.get_weight() + self.changeset_id.get_weight()
    }
}

impl Eq for HookFile {}

impl Hash for HookFile {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.path.hash(state);
        self.changeset_id.hash(state);
    }
}

impl HookFile {
    pub fn new(
        path: String,
        content_store: Arc<FileContentStore>,
        changeset_id: HgChangesetId,
        ty: ChangedFileType,
    ) -> HookFile {
        HookFile {
            path,
            content_store,
            changeset_id,
            ty,
        }
    }

    pub fn contains_string(&self, data: &str) -> BoxFuture<bool, Error> {
        let data = data.to_string();
        self.file_content()
            .and_then(move |bytes| {
                let str_content = str::from_utf8(&bytes)?.to_string();
                Ok(str_content.contains(&data))
            })
            .boxify()
    }

    pub fn len(&self) -> BoxFuture<u64, Error> {
        self.file_content()
            .and_then(|bytes| Ok(bytes.len() as u64))
            .boxify()
    }

    pub fn file_content(&self) -> BoxFuture<Bytes, Error> {
        let path = try_boxfuture!(MPath::new(self.path.as_bytes()));
        let changeset_id = self.changeset_id.clone();
        self.content_store
            .get_file_content_for_changeset(self.changeset_id, path.clone())
            .and_then(move |opt| {
                opt.ok_or(ErrorKind::NoFileContent(changeset_id, path.into()).into())
            })
            .map(|(_, bytes)| bytes)
            .boxify()
    }

    pub fn file_type(&self) -> BoxFuture<FileType, Error> {
        let path = try_boxfuture!(MPath::new(self.path.as_bytes()));
        let changeset_id = self.changeset_id.clone();
        self.content_store
            .get_file_content_for_changeset(self.changeset_id, path.clone())
            .and_then(move |opt| {
                opt.ok_or(ErrorKind::NoFileContent(changeset_id, path.into()).into())
            })
            .map(|(file_type, _)| file_type)
            .boxify()
    }
}

impl HookChangeset {
    pub fn new(
        author: String,
        files: Vec<HookFile>,
        comments: String,
        parents: HookChangesetParents,
        changeset_id: HgChangesetId,
        content_store: Arc<FileContentStore>,
    ) -> HookChangeset {
        HookChangeset {
            author,
            files,
            comments,
            parents,
            content_store,
            changeset_id,
        }
    }

    pub fn file_content(&self, path: String) -> BoxFuture<Option<Bytes>, Error> {
        let path = try_boxfuture!(MPath::new(path.as_bytes()));
        self.content_store
            .get_file_content_for_changeset(self.changeset_id, path.clone())
            .map(|opt| opt.map(|(_, bytes)| bytes))
            .boxify()
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

    fn get_changed_files(
        &self,
        changesetid: &HgChangesetId,
    ) -> BoxFuture<Vec<(String, ChangedFileType)>, Error>;
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

    fn get_changed_files(
        &self,
        changesetid: &HgChangesetId,
    ) -> BoxFuture<Vec<(String, ChangedFileType)>, Error> {
        cloned!(self.repo);
        self.repo
            .get_changeset_by_changesetid(changesetid)
            .and_then(move |cs| {
                let mf_id = cs.manifestid();
                let mf = repo.get_manifest_by_nodeid(&mf_id);
                let parents = cs.parents();
                let (maybe_p1, _) = parents.get_nodes();
                // TODO(stash): generate changed file stream correctly for merges
                let p_mf = match maybe_p1.cloned() {
                    Some(p1) => repo.get_changeset_by_changesetid(&HgChangesetId::new(p1))
                        .and_then({
                            cloned!(repo);
                            move |p1| repo.get_manifest_by_nodeid(&p1.manifestid())
                        })
                        .left_future(),
                    None => finished(get_empty_manifest()).right_future(),
                };
                (mf, p_mf)
            })
            .and_then(|(mf, p_mf)| {
                manifest_utils::changed_file_stream(&mf, &p_mf, None)
                    .map(|changed_entry| {
                        let path = changed_entry
                            .get_full_path()
                            .expect("File should have a path");
                        let ty = ChangedFileType::from(changed_entry.status);
                        (String::from_utf8_lossy(&path.to_vec()).into_owned(), ty)
                    })
                    .collect()
            })
            .boxify()
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

    fn get_changed_files(
        &self,
        changesetid: &HgChangesetId,
    ) -> BoxFuture<Vec<(String, ChangedFileType)>, Error> {
        match self.map.get(changesetid) {
            Some(cs) => Box::new(finished(
                cs.files()
                    .into_iter()
                    .map(|arr| String::from_utf8_lossy(&arr.to_vec()).into_owned())
                    .map(|path| (path, ChangedFileType::Added))
                    .collect(),
            )),
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
    ) -> BoxFuture<Option<(FileType, Bytes)>, Error>;
}

#[derive(Clone)]
pub struct InMemoryFileContentStore {
    map: HashMap<(HgChangesetId, MPath), (FileType, Bytes)>,
}

impl FileContentStore for InMemoryFileContentStore {
    fn get_file_content_for_changeset(
        &self,
        changesetid: HgChangesetId,
        path: MPath,
    ) -> BoxFuture<Option<(FileType, Bytes)>, Error> {
        let opt = self.map
            .get(&(changesetid, path.clone()))
            .map(|(file_type, bytes)| (file_type.clone(), bytes.clone()));
        finished(opt).boxify()
    }
}

impl InMemoryFileContentStore {
    pub fn new() -> InMemoryFileContentStore {
        InMemoryFileContentStore {
            map: HashMap::new(),
        }
    }

    pub fn insert(&mut self, key: (HgChangesetId, MPath), content: (FileType, Bytes)) {
        self.map.insert(key, content);
    }
}

// TODO this can cache file content locally to prevent unnecessary lookup of changeset,
// manifest and walk of manifest each time
// It's likely that multiple hooks will want to see the same content for the same changeset
pub struct BlobRepoFileContentStore {
    pub repo: BlobRepo,
}

impl FileContentStore for BlobRepoFileContentStore {
    fn get_file_content_for_changeset(
        &self,
        changesetid: HgChangesetId,
        path: MPath,
    ) -> BoxFuture<Option<(FileType, Bytes)>, Error> {
        let repo = self.repo.clone();
        let repo2 = repo.clone();
        repo.get_changeset_by_changesetid(&changesetid)
            .and_then(move |changeset| {
                repo.find_file_in_manifest(&path, changeset.manifestid().clone())
            })
            .and_then(move |opt| match opt {
                Some((file_type, hash)) => repo2
                    .get_file_content(&hash.into_nodehash())
                    .map(move |content| Some((file_type, content)))
                    .boxify(),
                None => finished(None).boxify(),
            })
            .and_then(|opt| match opt {
                Some((file_type, content)) => {
                    let FileContents::Bytes(bytes) = content;
                    Ok(Some((file_type, bytes)))
                }
                None => Ok(None),
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
}

impl Filler for HookCacheFiller {
    type Key = FileHookExecutionID;
    type Value = BoxFuture<HookExecution, Error>;

    fn fill(&self, _cache: &Asyncmemo<Self>, key: &Self::Key) -> Self::Value {
        let hooks = self.file_hooks.lock().unwrap();
        match hooks.get(&key.hook_name) {
            Some(arc_hook) => {
                let arc_hook = arc_hook.clone();
                let hook_context: HookContext<HookFile> = HookContext::new(
                    key.hook_name.clone(),
                    self.repo_name.clone(),
                    key.file.clone(),
                );
                arc_hook.0.run(hook_context)
            }
            None => panic!("Can't find hook {}", key.hook_name), // TODO
        }
    }
}

#[derive(Clone, Debug, PartialEq, Hash, Eq)]
// TODO Note that when we move to Bonsai changesets the ID that we use in the cache will
// be the content hash
pub struct FileHookExecutionID {
    pub cs_id: HgChangesetId,
    pub hook_name: String,
    pub file: HookFile,
}

#[derive(Clone, Debug, PartialEq, Hash, Eq)]
pub struct ChangesetHookExecutionID {
    pub cs_id: HgChangesetId,
    pub hook_name: String,
}

impl Weight for FileHookExecutionID {
    fn get_weight(&self) -> usize {
        self.cs_id.get_weight() + self.hook_name.get_weight() + self.file.get_weight()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum HookChangesetParents {
    None,
    One(String),
    Two(String, String),
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
    use fixtures::many_files_dirs;
    use futures::{stream, Stream};
    use futures::Future;
    use futures::future::finished;
    use slog::{Discard, Drain};
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
    struct ContainsStringMatchingChangesetHook {
        expected_content: HashMap<String, String>,
    }

    impl Hook<HookChangeset> for ContainsStringMatchingChangesetHook {
        fn run(&self, context: HookContext<HookChangeset>) -> BoxFuture<HookExecution, Error> {
            let mut futs = stream::FuturesUnordered::new();
            for file in context.data.files {
                let fut = match self.expected_content.get(&file.path) {
                    Some(content) => file.contains_string(&content),
                    None => Box::new(finished(false)),
                };
                futs.push(fut);
            }
            futs.skip_while(|b| Ok(*b))
                .into_future()
                .map(|(opt_item, _)| {
                    if opt_item.is_some() {
                        default_rejection()
                    } else {
                        HookExecution::Accepted
                    }
                })
                .map_err(|(e, _)| e)
                .boxify()
        }
    }

    fn contains_string_matching_changeset_hook(
        expected_content: HashMap<String, String>,
    ) -> Box<Hook<HookChangeset>> {
        Box::new(ContainsStringMatchingChangesetHook { expected_content })
    }

    #[derive(Clone, Debug)]
    struct FileContentMatchingChangesetHook {
        expected_content: HashMap<String, String>,
    }

    impl Hook<HookChangeset> for FileContentMatchingChangesetHook {
        fn run(&self, context: HookContext<HookChangeset>) -> BoxFuture<HookExecution, Error> {
            let mut futs = stream::FuturesUnordered::new();
            for file in context.data.files {
                let fut = match self.expected_content.get(&file.path) {
                    Some(expected_content) => {
                        let expected_content = expected_content.clone();
                        file.file_content()
                            .map(move |content| {
                                let content = str::from_utf8(&*content).unwrap().to_string();
                                content.contains(&expected_content)
                            })
                            .boxify()
                    }
                    None => Box::new(finished(false)),
                };
                futs.push(fut);
            }
            futs.skip_while(|b| Ok(*b))
                .into_future()
                .map(|(opt_item, _)| {
                    if opt_item.is_some() {
                        default_rejection()
                    } else {
                        HookExecution::Accepted
                    }
                })
                .map_err(|(e, _)| e)
                .boxify()
        }
    }

    fn file_content_matching_changeset_hook(
        expected_content: HashMap<String, String>,
    ) -> Box<Hook<HookChangeset>> {
        Box::new(FileContentMatchingChangesetHook { expected_content })
    }

    #[derive(Clone, Debug)]
    struct LengthMatchingChangesetHook {
        expected_lengths: HashMap<String, u64>,
    }

    impl Hook<HookChangeset> for LengthMatchingChangesetHook {
        fn run(&self, context: HookContext<HookChangeset>) -> BoxFuture<HookExecution, Error> {
            let mut futs = stream::FuturesUnordered::new();
            for file in context.data.files {
                let fut = match self.expected_lengths.get(&file.path) {
                    Some(expected_length) => {
                        let expected_length = *expected_length;
                        file.len()
                            .map(move |length| length == expected_length)
                            .boxify()
                    }
                    None => Box::new(finished(false)),
                };
                futs.push(fut);
            }
            futs.skip_while(|b| Ok(*b))
                .into_future()
                .map(|(opt_item, _)| {
                    if opt_item.is_some() {
                        default_rejection()
                    } else {
                        HookExecution::Accepted
                    }
                })
                .map_err(|(e, _)| e)
                .boxify()
        }
    }

    fn length_matching_changeset_hook(
        expected_lengths: HashMap<String, u64>,
    ) -> Box<Hook<HookChangeset>> {
        Box::new(LengthMatchingChangesetHook { expected_lengths })
    }

    #[derive(Clone, Debug)]
    struct OtherFileMatchingChangesetHook {
        file_path: String,
        expected_content: Option<String>,
    }

    impl Hook<HookChangeset> for OtherFileMatchingChangesetHook {
        fn run(&self, context: HookContext<HookChangeset>) -> BoxFuture<HookExecution, Error> {
            let expected_content = self.expected_content.clone();
            context
                .data
                .file_content(self.file_path.clone())
                .map(|opt| opt.map(|content| str::from_utf8(&*content).unwrap().to_string()))
                .map(move |opt| {
                    if opt == expected_content {
                        HookExecution::Accepted
                    } else {
                        default_rejection()
                    }
                })
                .boxify()
        }
    }

    fn other_file_matching_changeset_hook(
        file_path: String,
        expected_content: Option<String>,
    ) -> Box<Hook<HookChangeset>> {
        Box::new(OtherFileMatchingChangesetHook {
            file_path,
            expected_content,
        })
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
    struct ContainsStringMatchingFileHook {
        content: String,
    }

    impl Hook<HookFile> for ContainsStringMatchingFileHook {
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

    fn contains_string_matching_file_hook(content: String) -> Box<Hook<HookFile>> {
        Box::new(ContainsStringMatchingFileHook { content })
    }

    #[derive(Clone, Debug)]
    struct FileContentMatchingFileHook {
        content: String,
    }

    impl Hook<HookFile> for FileContentMatchingFileHook {
        fn run(&self, context: HookContext<HookFile>) -> BoxFuture<HookExecution, Error> {
            let expected_content = self.content.clone();
            context
                .data
                .file_content()
                .map(move |content| {
                    let content = str::from_utf8(&*content).unwrap().to_string();
                    if content.contains(&expected_content) {
                        HookExecution::Accepted
                    } else {
                        default_rejection()
                    }
                })
                .boxify()
        }
    }

    fn file_content_matching_file_hook(content: String) -> Box<Hook<HookFile>> {
        Box::new(FileContentMatchingFileHook { content })
    }

    #[derive(Clone, Debug)]
    struct IsSymLinkMatchingFileHook {
        is_symlink: bool,
    }

    impl Hook<HookFile> for IsSymLinkMatchingFileHook {
        fn run(&self, context: HookContext<HookFile>) -> BoxFuture<HookExecution, Error> {
            let is_symlink = self.is_symlink;
            context
                .data
                .file_type()
                .map(move |file_type| {
                    let actual = match file_type {
                        FileType::Symlink => true,
                        _ => false,
                    };
                    if is_symlink == actual {
                        HookExecution::Accepted
                    } else {
                        default_rejection()
                    }
                })
                .boxify()
        }
    }

    fn is_symlink_matching_file_hook(is_symlink: bool) -> Box<Hook<HookFile>> {
        Box::new(IsSymLinkMatchingFileHook { is_symlink })
    }

    #[derive(Clone, Debug)]
    struct LengthMatchingFileHook {
        length: u64,
    }

    impl Hook<HookFile> for LengthMatchingFileHook {
        fn run(&self, context: HookContext<HookFile>) -> BoxFuture<HookExecution, Error> {
            let exp_length = self.length;
            context
                .data
                .len()
                .map(move |length| {
                    if length == exp_length {
                        HookExecution::Accepted
                    } else {
                        default_rejection()
                    }
                })
                .boxify()
        }
    }

    fn length_matching_file_hook(length: u64) -> Box<Hook<HookFile>> {
        Box::new(LengthMatchingFileHook { length })
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
                "dir1/subdir1/subsubdir1/file_1".to_string(),
                "dir1/subdir1/subsubdir2/file_1".to_string(),
                "dir1/subdir1/subsubdir2/file_2".to_string(),
            ];
            let content_store = Arc::new(InMemoryFileContentStore::new());
            let cs_id = default_changeset_id();
            let hook_files = files
                .iter()
                .map(|path| {
                    HookFile::new(
                        path.clone(),
                        content_store.clone(),
                        cs_id,
                        ChangedFileType::Added,
                    )
                })
                .collect();
            let parents =
                HookChangesetParents::One("2f866e7e549760934e31bf0420a873f65100ad63".into());
            let data = HookChangeset::new(
                "Stanislau Hlebik <stash@fb.com>".into(),
                hook_files,
                "3".into(),
                parents,
                cs_id,
                content_store,
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
    fn test_changeset_hook_contains_string() {
        async_unit::tokio_unit_test(|| {
            let hook1_map = hashmap![
                "dir1/subdir1/subsubdir1/file_1".to_string() => "elephants".to_string(),
                "dir1/subdir1/subsubdir2/file_1".to_string() => "hippopatami".to_string(),
                "dir1/subdir1/subsubdir2/file_2".to_string() => "eels".to_string()
            ];
            let hook2_map = hashmap![
                "dir1/subdir1/subsubdir1/file_1".to_string() => "anteaters".to_string(),
                "dir1/subdir1/subsubdir2/file_1".to_string() => "hippopatami".to_string(),
                "dir1/subdir1/subsubdir2/file_2".to_string() => "eels".to_string()
            ];
            let hook3_map = hashmap![
                "dir1/subdir1/subsubdir1/file_1".to_string() => "anteaters".to_string(),
                "dir1/subdir1/subsubdir2/file_1".to_string() => "giraffes".to_string(),
                "dir1/subdir1/subsubdir2/file_2".to_string() => "lions".to_string()
            ];
            let hooks: HashMap<String, Box<Hook<HookChangeset>>> = hashmap! {
                "hook1".to_string() => contains_string_matching_changeset_hook(hook1_map),
                "hook2".to_string() => contains_string_matching_changeset_hook(hook2_map),
                "hook3".to_string() => contains_string_matching_changeset_hook(hook3_map),
            };
            let bookmarks = hashmap! {
                "bm1".to_string() => vec!["hook1".to_string(), "hook2".to_string(), "hook3".to_string()]
            };
            let expected = hashmap! {
                "hook1".to_string() => HookExecution::Accepted,
                "hook2".to_string() => default_rejection(),
                "hook3".to_string() => default_rejection(),
            };
            run_changeset_hooks("bm1", hooks, bookmarks, expected);
        });
    }

    #[test]
    fn test_changeset_hook_other_file_content() {
        async_unit::tokio_unit_test(|| {
            let hooks: HashMap<String, Box<Hook<HookChangeset>>> = hashmap! {
                "hook1".to_string() => other_file_matching_changeset_hook("dir1/subdir1/subsubdir1/file_1".to_string(), Some("elephants".to_string())),
                "hook2".to_string() => other_file_matching_changeset_hook("dir1/subdir1/subsubdir1/file_1".to_string(), Some("giraffes".to_string())),
                "hook3".to_string() => other_file_matching_changeset_hook("dir1/subdir1/subsubdir2/file_2".to_string(), Some("aardvarks".to_string())),
                "hook4".to_string() => other_file_matching_changeset_hook("no/such/path".to_string(), None),
                "hook5".to_string() => other_file_matching_changeset_hook("no/such/path".to_string(), Some("whateva".to_string())),
            };
            let bookmarks = hashmap! {
                "bm1".to_string() => vec!["hook1".to_string(), "hook2".to_string(), "hook3".to_string(), "hook4".to_string(), "hook5".to_string()]
            };
            let expected = hashmap! {
                "hook1".to_string() => HookExecution::Accepted,
                "hook2".to_string() => default_rejection(),
                "hook3".to_string() => default_rejection(),
                "hook4".to_string() => HookExecution::Accepted,
                "hook5".to_string() => default_rejection(),
            };
            run_changeset_hooks("bm1", hooks, bookmarks, expected);
        });
    }

    #[test]
    fn test_changeset_hook_file_content() {
        async_unit::tokio_unit_test(|| {
            let hook1_map = hashmap![
                "dir1/subdir1/subsubdir1/file_1".to_string() => "elephants".to_string(),
                "dir1/subdir1/subsubdir2/file_1".to_string() => "hippopatami".to_string(),
                "dir1/subdir1/subsubdir2/file_2".to_string() => "eels".to_string()
            ];
            let hook2_map = hashmap![
                "dir1/subdir1/subsubdir1/file_1".to_string() => "anteaters".to_string(),
                "dir1/subdir1/subsubdir2/file_1".to_string() => "hippopatami".to_string(),
                "dir1/subdir1/subsubdir2/file_2".to_string() => "eels".to_string()
            ];
            let hook3_map = hashmap![
                "dir1/subdir1/subsubdir1/file_1".to_string() => "anteaters".to_string(),
                "dir1/subdir1/subsubdir2/file_1".to_string() => "giraffes".to_string(),
                "dir1/subdir1/subsubdir2/file_2".to_string() => "lions".to_string()
            ];
            let hooks: HashMap<String, Box<Hook<HookChangeset>>> = hashmap! {
                "hook1".to_string() => file_content_matching_changeset_hook(hook1_map),
                "hook2".to_string() => file_content_matching_changeset_hook(hook2_map),
                "hook3".to_string() => file_content_matching_changeset_hook(hook3_map),
            };
            let bookmarks = hashmap! {
                "bm1".to_string() => vec!["hook1".to_string(), "hook2".to_string(), "hook3".to_string()]
            };
            let expected = hashmap! {
                "hook1".to_string() => HookExecution::Accepted,
                "hook2".to_string() => default_rejection(),
                "hook3".to_string() => default_rejection(),
            };
            run_changeset_hooks("bm1", hooks, bookmarks, expected);
        });
    }

    #[test]
    fn test_changeset_hook_lengths() {
        async_unit::tokio_unit_test(|| {
            let hook1_map = hashmap![
                "dir1/subdir1/subsubdir1/file_1".to_string() => 9,
                "dir1/subdir1/subsubdir2/file_1".to_string() => 11,
                "dir1/subdir1/subsubdir2/file_2".to_string() => 4
            ];
            let hook2_map = hashmap![
                "dir1/subdir1/subsubdir1/file_1".to_string() => 9,
                "dir1/subdir1/subsubdir2/file_1".to_string() => 12,
                "dir1/subdir1/subsubdir2/file_2".to_string() => 4
            ];
            let hook3_map = hashmap![
                "dir1/subdir1/subsubdir1/file_1".to_string() => 15,
                "dir1/subdir1/subsubdir2/file_1".to_string() => 17,
                "dir1/subdir1/subsubdir2/file_2".to_string() => 2
            ];
            let hooks: HashMap<String, Box<Hook<HookChangeset>>> = hashmap! {
                "hook1".to_string() => length_matching_changeset_hook(hook1_map),
                "hook2".to_string() => length_matching_changeset_hook(hook2_map),
                "hook3".to_string() => length_matching_changeset_hook(hook3_map),
            };
            let bookmarks = hashmap! {
                "bm1".to_string() => vec!["hook1".to_string(),
                 "hook2".to_string(), "hook3".to_string()
                 ]
            };
            let expected = hashmap! {
                "hook1".to_string() => HookExecution::Accepted,
                "hook2".to_string() => default_rejection(),
                "hook3".to_string() => default_rejection(),
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
    fn test_file_hook_contains_string() {
        async_unit::tokio_unit_test(|| {
            let hooks: HashMap<String, Box<Hook<HookFile>>> = hashmap! {
                "hook1".to_string() => contains_string_matching_file_hook("elephants".to_string()),
                "hook2".to_string() => contains_string_matching_file_hook("hippopatami".to_string()),
                "hook3".to_string() => contains_string_matching_file_hook("eels".to_string())
            };
            let bookmarks = hashmap! {
                "bm1".to_string() => vec!["hook1".to_string(),
                "hook2".to_string(), "hook3".to_string()
            ]
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
    fn test_file_hook_file_content() {
        async_unit::tokio_unit_test(|| {
            let hooks: HashMap<String, Box<Hook<HookFile>>> = hashmap! {
                "hook1".to_string() => file_content_matching_file_hook("elephants".to_string()),
                "hook2".to_string() => file_content_matching_file_hook("hippopatami".to_string()),
                "hook3".to_string() => file_content_matching_file_hook("eels".to_string())
            };
            let bookmarks = hashmap! {
                "bm1".to_string() => vec!["hook1".to_string(),
                "hook2".to_string(), "hook3".to_string()
            ]
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
    fn test_file_hook_is_symlink() {
        async_unit::tokio_unit_test(|| {
            let hooks: HashMap<String, Box<Hook<HookFile>>> = hashmap! {
                "hook1".to_string() => is_symlink_matching_file_hook(true),
                "hook2".to_string() => is_symlink_matching_file_hook(false),
            };
            let bookmarks = hashmap! {
                "bm1".to_string() => vec!["hook1".to_string(),
                "hook2".to_string()
            ]
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
                    "dir1/subdir1/subsubdir2/file_2".to_string() => HookExecution::Accepted,
                },
            };
            run_file_hooks("bm1", hooks, bookmarks, expected);
        });
    }

    #[test]
    fn test_file_hook_length() {
        async_unit::tokio_unit_test(|| {
            let hooks: HashMap<String, Box<Hook<HookFile>>> = hashmap! {
                "hook1".to_string() => length_matching_file_hook("elephants".len() as u64),
                "hook2".to_string() => length_matching_file_hook("hippopatami".len() as u64),
                "hook3".to_string() => length_matching_file_hook("eels".len() as u64),
                "hook4".to_string() => length_matching_file_hook(999)
            };
            let bookmarks = hashmap! {
                "bm1".to_string() => vec!["hook1".to_string(),
                "hook2".to_string(), "hook3".to_string(), "hook4".to_string()
                ]
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
                "hook4".to_string() => hashmap! {
                    "dir1/subdir1/subsubdir1/file_1".to_string() => default_rejection(),
                    "dir1/subdir1/subsubdir2/file_1".to_string() => default_rejection(),
                    "dir1/subdir1/subsubdir2/file_2".to_string() => default_rejection(),
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
            hook_manager.register_changeset_hook("hook1", hook1.into(), None);
            let hook2 = always_accepting_changeset_hook();
            hook_manager.register_changeset_hook("hook2", hook2.into(), None);

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
            hook_manager.register_changeset_hook(&hook_name, hook.into(), None);
        }
        let fut = hook_manager.run_changeset_hooks_for_bookmark(
            default_changeset_id(),
            &Bookmark::new(bookmark_name).unwrap(),
            None,
        );
        let res = fut.wait().unwrap();
        let map: HashMap<String, HookExecution> = res.into_iter()
            .map(|(exec_id, exec)| (exec_id.hook_name, exec))
            .collect();
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
            hook_manager.register_file_hook(&hook_name, hook.into(), None);
        }
        let fut: BoxFuture<Vec<(FileHookExecutionID, HookExecution)>, Error> = hook_manager
            .run_file_hooks_for_bookmark(
                default_changeset_id(),
                &Bookmark::new(bookmark_name).unwrap(),
                None,
            );
        let res = fut.wait().unwrap();
        let map: HashMap<String, HashMap<String, HookExecution>> = res.into_iter().fold(
            HashMap::new(),
            |mut m, (exec_id, exec)| {
                match m.entry(exec_id.hook_name) {
                    Entry::Vacant(v) => v.insert(HashMap::new()).insert(exec_id.file.path, exec),
                    Entry::Occupied(mut v) => v.get_mut().insert(exec_id.file.path, exec),
                };
                m
            },
        );
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
        HgChangesetId::from_str("d261bc7900818dea7c86935b3fb17a33b2e3a6b4").unwrap()
    }

    fn hook_manager_blobrepo() -> HookManager {
        let repo = many_files_dirs::getrepo(None);
        let changeset_store = BlobRepoChangesetStore::new(repo.clone());
        let content_store = BlobRepoFileContentStore::new(repo);
        let logger = Logger::root(Discard {}.ignore_res(), o!());
        HookManager::new(
            "some_repo".into(),
            Box::new(changeset_store),
            Arc::new(content_store),
            1024,
            1024 * 1024,
            logger,
        )
    }

    fn hook_manager_inmem() -> HookManager {
        let repo = many_files_dirs::getrepo(None);
        // Load up an in memory store with a single commit from the many_files_dirs store
        let cs_id = HgChangesetId::from_str("d261bc7900818dea7c86935b3fb17a33b2e3a6b4").unwrap();
        let cs = repo.get_changeset_by_changesetid(&cs_id).wait().unwrap();
        let mut changeset_store = InMemoryChangesetStore::new();
        changeset_store.insert(&cs_id, &cs);
        let mut content_store = InMemoryFileContentStore::new();
        content_store.insert(
            (cs_id.clone(), to_mpath("dir1/subdir1/subsubdir1/file_1")),
            (FileType::Symlink, "elephants".into()),
        );
        content_store.insert(
            (cs_id.clone(), to_mpath("dir1/subdir1/subsubdir2/file_1")),
            (FileType::Regular, "hippopatami".into()),
        );
        content_store.insert(
            (cs_id.clone(), to_mpath("dir1/subdir1/subsubdir2/file_2")),
            (FileType::Regular, "eels".into()),
        );
        let logger = Logger::root(Discard {}.ignore_res(), o!());
        HookManager::new(
            "some_repo".into(),
            Box::new(changeset_store),
            Arc::new(content_store),
            1024,
            1024 * 1024,
            logger,
        )
    }

    pub fn to_mpath(string: &str) -> MPath {
        // Please... avert your eyes
        MPath::new(string.to_string().as_bytes().to_vec()).unwrap()
    }

}
