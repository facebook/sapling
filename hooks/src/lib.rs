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

pub mod errors;
mod facebook;
pub mod hook_loader;
pub mod lua_hook;
mod phabricator_message_parser;
pub mod rust_hook;

use aclchecker::{AclChecker, Identity};
use asyncmemo::{Asyncmemo, Filler, Weight};
use bookmarks::BookmarkName;
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
pub use errors::*;
use failure_ext::{err_msg, Error, FutureFailureErrorExt};
use futures::{finished, future, Future, IntoFuture};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use mercurial_types::{
    blobs::HgBlobChangeset, manifest_utils::EntryStatus, Changeset, HgChangesetId, HgFileNodeId,
    HgParents, MPath,
};
use metaconfig_types::{BookmarkOrRegex, HookBypass, HookConfig, HookManagerParams};
use mononoke_types::FileType;
use regex::Regex;
use slog::{debug, Logger};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::mem;
use std::str;
use std::sync::{Arc, Mutex};
use tracing::{trace_args, Traced};

type ChangesetHooks = HashMap<String, (Arc<dyn Hook<HookChangeset>>, HookConfig)>;
type FileHooks = Arc<Mutex<HashMap<String, (Arc<dyn Hook<HookFile>>, HookConfig)>>>;
type Cache = Asyncmemo<HookCacheFiller>;

/// Manages hooks and allows them to be installed and uninstalled given a name
/// Knows how to run hooks

pub struct HookManager {
    cache: Cache,
    changeset_hooks: ChangesetHooks,
    file_hooks: FileHooks,
    bookmark_hooks: HashMap<BookmarkName, Vec<String>>,
    regex_hooks: Vec<(Regex, Vec<String>)>,
    changeset_store: Box<dyn ChangesetStore>,
    content_store: Arc<dyn FileContentStore>,
    logger: Logger,
    reviewers_acl_checker: Arc<Option<AclChecker>>,
}

impl HookManager {
    pub fn new(
        ctx: CoreContext,
        changeset_store: Box<dyn ChangesetStore>,
        content_store: Arc<dyn FileContentStore>,
        hook_manager_params: HookManagerParams,
        logger: Logger,
    ) -> HookManager {
        let changeset_hooks = HashMap::new();
        let file_hooks = Arc::new(Mutex::new(HashMap::new()));

        let filler = HookCacheFiller {
            ctx,
            file_hooks: file_hooks.clone(),
        };
        let cache = Asyncmemo::with_limits(
            "hooks",
            filler,
            hook_manager_params.entrylimit,
            hook_manager_params.weightlimit,
        );

        let reviewers_acl_checker = if !hook_manager_params.disable_acl_checker {
            let identity = Identity::from_groupname(facebook::REVIEWERS_ACL_GROUP_NAME);

            // This can block, but not too big a deal as we create hook manager in server startup
            AclChecker::new(&identity)
                .and_then(|reviewers_acl_checker| {
                    if reviewers_acl_checker.do_wait_updated(10000) {
                        Ok(reviewers_acl_checker)
                    } else {
                        Err(err_msg("did not update acl checker"))
                    }
                })
                .ok()
        } else {
            None
        };

        HookManager {
            cache,
            changeset_hooks,
            file_hooks,
            bookmark_hooks: HashMap::new(),
            regex_hooks: Vec::new(),
            changeset_store,
            content_store,
            logger,
            reviewers_acl_checker: Arc::new(reviewers_acl_checker),
        }
    }

    pub fn register_changeset_hook(
        &mut self,
        hook_name: &str,
        hook: Arc<dyn Hook<HookChangeset>>,
        config: HookConfig,
    ) {
        self.changeset_hooks
            .insert(hook_name.to_string(), (hook, config));
    }

    pub fn register_file_hook(
        &mut self,
        hook_name: &str,
        hook: Arc<dyn Hook<HookFile>>,
        config: HookConfig,
    ) {
        let mut hooks = self.file_hooks.lock().unwrap();
        hooks.insert(hook_name.to_string(), (hook, config));
    }

    pub fn set_hooks_for_bookmark(&mut self, bookmark: BookmarkOrRegex, hooks: Vec<String>) {
        match bookmark {
            BookmarkOrRegex::Bookmark(bookmark) => {
                self.bookmark_hooks.insert(bookmark, hooks);
            }
            BookmarkOrRegex::Regex(regex) => {
                self.regex_hooks.push((regex, hooks));
            }
        }
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

    fn hooks_for_bookmark(&self, bookmark: &BookmarkName) -> HashSet<String> {
        let mut hooks: HashSet<_> = match self.bookmark_hooks.get(bookmark) {
            Some(hooks) => hooks.clone().into_iter().collect(),
            None => HashSet::new(),
        };

        let bookmark_str = bookmark.to_string();
        for (regex, r_hooks) in &self.regex_hooks {
            if regex.is_match(&bookmark_str) {
                hooks.extend(r_hooks.iter().cloned());
            }
        }

        hooks
    }

    // Changeset hooks

    pub fn run_changeset_hooks_for_bookmark(
        &self,
        ctx: CoreContext,
        changeset_id: HgChangesetId,
        bookmark: &BookmarkName,
        maybe_pushvars: Option<HashMap<String, Bytes>>,
    ) -> BoxFuture<Vec<(ChangesetHookExecutionID, HookExecution)>, Error> {
        let hooks: Vec<_> = self
            .hooks_for_bookmark(bookmark)
            .into_iter()
            .filter(|name| self.changeset_hooks.contains_key(name))
            .collect();

        if hooks.is_empty() {
            finished(Vec::new()).boxify()
        } else {
            self.run_changeset_hooks_for_changeset_id(
                ctx,
                changeset_id,
                hooks,
                maybe_pushvars,
                bookmark,
            )
        }
    }

    fn run_changeset_hooks_for_changeset_id(
        &self,
        ctx: CoreContext,
        changeset_id: HgChangesetId,
        hooks: Vec<String>,
        maybe_pushvars: Option<HashMap<String, Bytes>>,
        bookmark: &BookmarkName,
    ) -> BoxFuture<Vec<(ChangesetHookExecutionID, HookExecution)>, Error> {
        let hooks: Result<Vec<(String, (Arc<dyn Hook<HookChangeset>>, _))>, Error> = hooks
            .iter()
            .map(|hook_name| {
                let hook = self
                    .changeset_hooks
                    .get(hook_name)
                    .ok_or(ErrorKind::NoSuchHook(hook_name.to_string()))?;
                Ok((hook_name.clone(), hook.clone()))
            })
            .collect();
        let hooks = try_boxfuture!(hooks);
        self.get_hook_changeset(ctx.clone(), changeset_id)
            .and_then({
                cloned!(bookmark);
                move |hcs| {
                    let hooks = HookManager::filter_bypassed_hooks(
                        hooks,
                        &hcs.comments,
                        maybe_pushvars.as_ref(),
                    );

                    HookManager::run_changeset_hooks_for_changeset(
                        ctx,
                        hcs.clone(),
                        hooks.clone(),
                        bookmark,
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
        ctx: CoreContext,
        changeset: HookChangeset,
        hooks: Vec<(String, Arc<dyn Hook<HookChangeset>>, HookConfig)>,
        bookmark: BookmarkName,
    ) -> BoxFuture<Vec<(String, HookExecution)>, Error> {
        futures::future::join_all(hooks.into_iter().map(move |(hook_name, hook, config)| {
            HookManager::run_changeset_hook(
                ctx.clone(),
                hook,
                HookContext::new(hook_name, config, changeset.clone(), bookmark.clone()),
            )
        }))
        .boxify()
    }

    fn run_changeset_hook(
        ctx: CoreContext,
        hook: Arc<dyn Hook<HookChangeset>>,
        hook_context: HookContext<HookChangeset>,
    ) -> BoxFuture<(String, HookExecution), Error> {
        let hook_name = hook_context.hook_name.clone();
        hook.run(ctx, hook_context)
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
        ctx: CoreContext,
        changeset_id: HgChangesetId,
        bookmark: &BookmarkName,
        maybe_pushvars: Option<HashMap<String, Bytes>>,
    ) -> BoxFuture<Vec<(FileHookExecutionID, HookExecution)>, Error> {
        debug!(
            self.logger.clone(),
            "Running file hooks for bookmark {:?}", bookmark
        );
        let hooks: Vec<_> = {
            let hooks = self.hooks_for_bookmark(bookmark);
            let file_hooks = self.file_hooks.lock().unwrap();
            hooks
                .into_iter()
                .filter_map(|name| file_hooks.get(&name).map(|hook| (name, hook.clone())))
                .collect()
        };

        if hooks.is_empty() {
            finished(Vec::new()).boxify()
        } else {
            self.run_file_hooks_for_changeset_id(
                ctx.clone(),
                changeset_id,
                hooks,
                maybe_pushvars,
                self.logger.clone(),
                bookmark.clone(),
            )
            .traced(
                &ctx.trace(),
                "run_file_hooks",
                trace_args! {
                    "bookmark" => bookmark.to_string(),
                    "changeset" => changeset_id.to_hex().to_string(),
                },
            )
            .boxify()
        }
    }

    fn run_file_hooks_for_changeset_id(
        &self,
        ctx: CoreContext,
        changeset_id: HgChangesetId,
        hooks: Vec<(String, (Arc<dyn Hook<HookFile>>, HookConfig))>,
        maybe_pushvars: Option<HashMap<String, Bytes>>,
        logger: Logger,
        bookmark: BookmarkName,
    ) -> BoxFuture<Vec<(FileHookExecutionID, HookExecution)>, Error> {
        debug!(
            self.logger,
            "Running file hooks for changeset id {:?}", changeset_id
        );
        let cache = self.cache.clone();
        self.get_hook_changeset(ctx.clone(), changeset_id)
            .and_then(move |hcs| {
                let hooks = HookManager::filter_bypassed_hooks(
                    hooks.clone(),
                    &hcs.comments,
                    maybe_pushvars.as_ref(),
                );
                let hooks = hooks.into_iter().map(|(name, _, _)| name).collect();

                HookManager::run_file_hooks_for_changeset(
                    changeset_id,
                    hcs.clone(),
                    hooks,
                    cache,
                    logger,
                    bookmark,
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
        bookmark: BookmarkName,
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
                            bookmark.clone(),
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
        bookmark: BookmarkName,
    ) -> BoxFuture<Vec<(FileHookExecutionID, HookExecution)>, Error> {
        let v: Vec<BoxFuture<(FileHookExecutionID, HookExecution), _>> = hooks
            .iter()
            .map(move |hook_name| {
                HookManager::run_file_hook(
                    FileHookExecutionID {
                        cs_id,
                        hook_name: hook_name.to_string(),
                        file: file.clone(),
                        bookmark: bookmark.clone(),
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

    fn get_hook_changeset(
        &self,
        ctx: CoreContext,
        changeset_id: HgChangesetId,
    ) -> BoxFuture<HookChangeset, Error> {
        let content_store = self.content_store.clone();
        let hg_changeset = self
            .changeset_store
            .get_changeset_by_changesetid(ctx.clone(), changeset_id);
        let changed_files = self.changeset_store.get_changed_files(ctx, changeset_id);
        let reviewers_acl_checker = self.reviewers_acl_checker.clone();
        Box::new((hg_changeset, changed_files).into_future().and_then(
            move |(changeset, changed_files)| {
                let author = str::from_utf8(changeset.user())?.into();
                let files = changed_files
                    .into_iter()
                    .map(|(path, ty, hash_and_type)| {
                        HookFile::new(
                            path,
                            content_store.clone(),
                            changeset_id.clone(),
                            ty,
                            hash_and_type,
                        )
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
                    reviewers_acl_checker,
                ))
            },
        ))
    }

    fn filter_bypassed_hooks<T: Clone>(
        hooks: Vec<(String, (T, HookConfig))>,
        commit_msg: &String,
        maybe_pushvars: Option<&HashMap<String, Bytes>>,
    ) -> Vec<(String, T, HookConfig)> {
        hooks
            .clone()
            .into_iter()
            .filter_map(|(hook_name, (hook, config))| {
                let maybe_bypassed_hook = match config.bypass {
                    Some(ref bypass) => {
                        if HookManager::is_hook_bypassed(bypass, commit_msg, maybe_pushvars) {
                            None
                        } else {
                            Some(())
                        }
                    }
                    None => Some(()),
                };
                maybe_bypassed_hook.map(move |()| (hook_name, hook, config))
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
    fn run(
        &self,
        ctx: CoreContext,
        hook_context: HookContext<T>,
    ) -> BoxFuture<HookExecution, Error>;
}

/// Represents a changeset - more user friendly than the blob changeset
/// as this uses String not Vec[u8]
#[derive(Clone)]
pub struct HookChangeset {
    pub author: String,
    pub files: Vec<HookFile>,
    pub comments: String,
    pub parents: HookChangesetParents,
    content_store: Arc<dyn FileContentStore>,
    changeset_id: HgChangesetId,
    reviewers_acl_checker: Arc<Option<AclChecker>>,
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
    content_store: Arc<dyn FileContentStore>,
    changeset_id: HgChangesetId,
    ty: ChangedFileType,
    hash_and_type: Option<(HgFileNodeId, FileType)>,
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
        content_store: Arc<dyn FileContentStore>,
        changeset_id: HgChangesetId,
        ty: ChangedFileType,
        hash_and_type: Option<(HgFileNodeId, FileType)>,
    ) -> HookFile {
        HookFile {
            path,
            content_store,
            changeset_id,
            ty,
            hash_and_type,
        }
    }

    pub fn contains_string(&self, ctx: CoreContext, data: &str) -> BoxFuture<bool, Error> {
        let data = data.to_string();
        self.file_content(ctx)
            .and_then(move |bytes| {
                let str_content = str::from_utf8(&bytes)?.to_string();
                Ok(str_content.contains(&data))
            })
            .boxify()
    }

    pub fn len(&self, ctx: CoreContext) -> BoxFuture<u64, Error> {
        let path = try_boxfuture!(MPath::new(self.path.as_bytes()));
        match self.hash_and_type {
            Some((entry_id, _)) => self.content_store.get_file_size(ctx, entry_id).boxify(),
            None => {
                future::err(ErrorKind::MissingFile(self.changeset_id, path.into()).into()).boxify()
            }
        }
    }

    pub fn file_content(&self, ctx: CoreContext) -> BoxFuture<Bytes, Error> {
        let path = try_boxfuture!(MPath::new(self.path.as_bytes()));
        match self.hash_and_type {
            Some((entry_id, _)) => self
                .content_store
                .get_file_content_by_id(ctx, entry_id)
                .boxify(),
            None => {
                future::err(ErrorKind::MissingFile(self.changeset_id, path.into()).into()).boxify()
            }
        }
    }

    pub fn file_type(&self, _ctx: CoreContext) -> BoxFuture<FileType, Error> {
        let path = try_boxfuture!(MPath::new(self.path.as_bytes()));
        cloned!(self.changeset_id);

        self.hash_and_type
            .ok_or(ErrorKind::MissingFile(changeset_id, path.into()).into())
            .into_future()
            .map(|(_, file_type)| file_type)
            .boxify()
    }

    pub fn changed_file_type(&self) -> ChangedFileType {
        self.ty.clone()
    }
}

impl HookChangeset {
    pub fn new(
        author: String,
        files: Vec<HookFile>,
        comments: String,
        parents: HookChangesetParents,
        changeset_id: HgChangesetId,
        content_store: Arc<dyn FileContentStore>,
        reviewers_acl_checker: Arc<Option<AclChecker>>,
    ) -> HookChangeset {
        HookChangeset {
            author,
            files,
            comments,
            parents,
            content_store,
            changeset_id,
            reviewers_acl_checker,
        }
    }

    pub fn file_content(&self, ctx: CoreContext, path: String) -> BoxFuture<Option<Bytes>, Error> {
        let path = try_boxfuture!(MPath::new(path.as_bytes()));
        self.content_store
            .get_file_content(ctx, self.changeset_id, path.clone())
            .boxify()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum HookExecution {
    Accepted,
    Rejected(HookRejectionInfo),
}

impl fmt::Display for HookExecution {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            HookExecution::Accepted => write!(f, "Accepted"),
            HookExecution::Rejected(reason) => write!(f, "Rejected: {}", reason.description),
        }
    }
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
    /// A short description for summarizing this failure with similar failures
    pub description: String,
    /// A full explanation of what went wrong, suitable for presenting to the user (should include guidance for fixing this failure, where possible)
    pub long_description: String,
}

impl Weight for HookRejectionInfo {
    fn get_weight(&self) -> usize {
        mem::size_of::<Self>() + self.description.get_weight() + self.long_description.get_weight()
    }
}

impl HookRejectionInfo {
    pub fn new(description: String) -> Self {
        Self::new_long(description.clone(), description)
    }

    pub fn new_opt(description: String, long_description: Option<String>) -> Self {
        let long_description = long_description.unwrap_or(description.clone());
        Self::new_long(description, long_description)
    }

    pub fn new_long(description: String, long_description: String) -> Self {
        Self {
            description,
            long_description,
        }
    }
}

pub trait ChangesetStore: Send + Sync {
    fn get_changeset_by_changesetid(
        &self,
        ctx: CoreContext,
        changesetid: HgChangesetId,
    ) -> BoxFuture<HgBlobChangeset, Error>;

    fn get_changed_files(
        &self,
        ctx: CoreContext,
        changesetid: HgChangesetId,
    ) -> BoxFuture<Vec<(String, ChangedFileType, Option<(HgFileNodeId, FileType)>)>, Error>;
}

pub struct InMemoryChangesetStore {
    map_files:
        HashMap<HgChangesetId, Vec<(String, ChangedFileType, Option<(HgFileNodeId, FileType)>)>>,
    map_cs: HashMap<HgChangesetId, HgBlobChangeset>,
}

impl ChangesetStore for InMemoryChangesetStore {
    fn get_changeset_by_changesetid(
        &self,
        _ctx: CoreContext,
        changesetid: HgChangesetId,
    ) -> BoxFuture<HgBlobChangeset, Error> {
        match self.map_cs.get(&changesetid) {
            Some(cs) => Box::new(future::ok(cs.clone())),
            None => Box::new(future::err(
                ErrorKind::NoSuchChangeset(changesetid.to_string()).into(),
            )),
        }
    }

    fn get_changed_files(
        &self,
        _ctx: CoreContext,
        changesetid: HgChangesetId,
    ) -> BoxFuture<Vec<(String, ChangedFileType, Option<(HgFileNodeId, FileType)>)>, Error> {
        match self.map_files.get(&changesetid) {
            Some(files) => future::ok(files.clone()).boxify(),
            None => Box::new(future::err(
                ErrorKind::NoSuchChangeset(changesetid.to_string()).into(),
            )),
        }
    }
}

impl InMemoryChangesetStore {
    pub fn new() -> InMemoryChangesetStore {
        InMemoryChangesetStore {
            map_cs: HashMap::new(),
            map_files: HashMap::new(),
        }
    }

    pub fn insert_files(
        &mut self,
        changeset_id: HgChangesetId,
        files: Vec<(String, ChangedFileType, Option<(HgFileNodeId, FileType)>)>,
    ) {
        self.map_files.insert(changeset_id.clone(), files);
    }

    pub fn insert_changeset(&mut self, changeset_id: HgChangesetId, cs: HgBlobChangeset) {
        self.map_cs.insert(changeset_id.clone(), cs);
    }
}

pub trait FileContentStore: Send + Sync {
    fn get_file_content(
        &self,
        ctx: CoreContext,
        changesetid: HgChangesetId,
        path: MPath,
    ) -> BoxFuture<Option<Bytes>, Error>;

    fn get_file_content_by_id(
        &self,
        ctx: CoreContext,
        entry_id: HgFileNodeId,
    ) -> BoxFuture<Bytes, Error>;

    fn get_file_size(&self, ctx: CoreContext, entry_id: HgFileNodeId) -> BoxFuture<u64, Error>;
}

#[derive(Clone)]
pub struct InMemoryFileContentStore {
    entry_id_to_bytes: HashMap<HgFileNodeId, Bytes>,
    path_to_bytes: HashMap<(HgChangesetId, MPath), Bytes>,
}

impl FileContentStore for InMemoryFileContentStore {
    fn get_file_content(
        &self,
        _ctx: CoreContext,
        cs_id: HgChangesetId,
        path: MPath,
    ) -> BoxFuture<Option<Bytes>, Error> {
        future::ok(self.path_to_bytes.get(&(cs_id, path)).cloned()).boxify()
    }

    fn get_file_content_by_id(
        &self,
        _ctx: CoreContext,
        hash: HgFileNodeId,
    ) -> BoxFuture<Bytes, Error> {
        self.entry_id_to_bytes
            .get(&hash)
            .map(|bytes| bytes.clone())
            .ok_or(err_msg("file not found"))
            .into_future()
            .boxify()
    }

    fn get_file_size(&self, _ctx: CoreContext, hash: HgFileNodeId) -> BoxFuture<u64, Error> {
        self.entry_id_to_bytes
            .get(&hash)
            .map(|bytes| bytes.len() as u64)
            .ok_or(err_msg("file not found"))
            .into_future()
            .boxify()
    }
}

impl InMemoryFileContentStore {
    pub fn new() -> InMemoryFileContentStore {
        InMemoryFileContentStore {
            entry_id_to_bytes: HashMap::new(),
            path_to_bytes: HashMap::new(),
        }
    }

    pub fn insert(&mut self, cs_id: HgChangesetId, path: MPath, key: HgFileNodeId, content: Bytes) {
        self.entry_id_to_bytes.insert(key, content.clone());
        self.path_to_bytes.insert((cs_id, path), content);
    }
}

struct HookCacheFiller {
    ctx: CoreContext,
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
                    arc_hook.1.clone(),
                    key.file.clone(),
                    key.bookmark.clone(),
                );
                arc_hook.0.run(self.ctx.clone(), hook_context)
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
    pub bookmark: BookmarkName,
}

#[derive(Clone, Debug, PartialEq, Hash, Eq)]
pub struct ChangesetHookExecutionID {
    pub cs_id: HgChangesetId,
    pub hook_name: String,
}

impl Weight for FileHookExecutionID {
    fn get_weight(&self) -> usize {
        self.cs_id.get_weight()
            + self.hook_name.get_weight()
            + self.file.get_weight()
            + self.bookmark.get_weight()
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
    pub config: HookConfig,
    pub data: T,
    pub bookmark: BookmarkName,
}

impl<T> HookContext<T>
where
    T: Clone,
{
    fn new(
        hook_name: String,
        config: HookConfig,
        data: T,
        bookmark: BookmarkName,
    ) -> HookContext<T> {
        HookContext {
            hook_name,
            config,
            data,
            bookmark,
        }
    }
}
