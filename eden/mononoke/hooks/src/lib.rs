/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

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
mod phabricator_message_parser;
pub mod rust_hook;

use aclchecker::{AclChecker, Identity};
use anyhow::{bail, Error};
use async_trait::async_trait;
use bookmarks::BookmarkName;
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
pub use errors::*;
use fbinit::FacebookInit;
use futures::{
    future::{try_join, try_join_all},
    stream::{futures_unordered::FuturesUnordered, TryStreamExt},
    Future, TryFutureExt,
};
use futures_stats::TimedFutureExt;
use hooks_content_stores::{ChangedFileType, ChangesetStore, FileContentStore};
use mercurial_types::{FileBytes, HgChangesetId, HgFileNodeId, HgParents, MPath};
use metaconfig_types::{BookmarkOrRegex, HookBypass, HookConfig, HookManagerParams};
use mononoke_types::FileType;
use regex::Regex;
use scuba::builder::ServerData;
use scuba_ext::ScubaSampleBuilder;
use slog::debug;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::str;
use std::sync::Arc;

type ChangesetHooks = HashMap<String, (Arc<dyn Hook<HookChangeset>>, HookConfig)>;
type FileHooks = HashMap<String, (Arc<dyn Hook<HookFile>>, HookConfig)>;

/// Manages hooks and allows them to be installed and uninstalled given a name
/// Knows how to run hooks

pub struct HookManager {
    changeset_hooks: ChangesetHooks,
    file_hooks: FileHooks,
    bookmark_hooks: HashMap<BookmarkName, Vec<String>>,
    regex_hooks: Vec<(Regex, Vec<String>)>,
    changeset_store: Box<dyn ChangesetStore>,
    content_store: Arc<dyn FileContentStore>,
    reviewers_acl_checker: Arc<Option<AclChecker>>,
    scuba: ScubaSampleBuilder,
}

impl HookManager {
    pub fn new(
        fb: FacebookInit,
        changeset_store: Box<dyn ChangesetStore>,
        content_store: Arc<dyn FileContentStore>,
        hook_manager_params: HookManagerParams,
        mut scuba: ScubaSampleBuilder,
    ) -> HookManager {
        let changeset_hooks = HashMap::new();
        let file_hooks = HashMap::new();

        scuba
            .add("driver", "mononoke")
            .add("scm", "hg")
            .add_mapped_common_server_data(|data| match data {
                ServerData::Hostname => "host",
                _ => data.default_key(),
            });

        let reviewers_acl_checker = if !hook_manager_params.disable_acl_checker {
            let identity = Identity::from_groupname(facebook::REVIEWERS_ACL_GROUP_NAME);

            // This can block, but not too big a deal as we create hook manager in server startup
            AclChecker::new(fb, &identity)
                .and_then(|reviewers_acl_checker| {
                    if reviewers_acl_checker.do_wait_updated(10000) {
                        Ok(reviewers_acl_checker)
                    } else {
                        bail!("did not update acl checker")
                    }
                })
                .ok()
        } else {
            None
        };

        HookManager {
            changeset_hooks,
            file_hooks,
            bookmark_hooks: HashMap::new(),
            regex_hooks: Vec::new(),
            changeset_store,
            content_store,
            reviewers_acl_checker: Arc::new(reviewers_acl_checker),
            scuba,
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
        self.file_hooks
            .insert(hook_name.to_string(), (hook, config));
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
            .iter()
            .map(|(name, _)| name.clone())
            .collect()
    }

    fn hooks_for_bookmark(&self, bookmark: &BookmarkName) -> Vec<String> {
        let mut hooks: Vec<_> = match self.bookmark_hooks.get(bookmark) {
            Some(hooks) => hooks.clone().into_iter().collect(),
            None => Vec::new(),
        };

        let bookmark_str = bookmark.to_string();
        for (regex, r_hooks) in &self.regex_hooks {
            if regex.is_match(&bookmark_str) {
                hooks.extend(r_hooks.iter().cloned());
            }
        }

        hooks
    }

    fn file_hooks_for_bookmark(&self, bookmark: &BookmarkName) -> Vec<String> {
        self.hooks_for_bookmark(bookmark)
            .into_iter()
            .filter(|name| self.file_hooks.contains_key(name))
            .collect()
    }

    fn changeset_hooks_for_bookmark(&self, bookmark: &BookmarkName) -> Vec<String> {
        self.hooks_for_bookmark(bookmark)
            .into_iter()
            .filter(|name| self.changeset_hooks.contains_key(name))
            .collect()
    }

    pub async fn run_hooks_for_bookmark(
        &self,
        ctx: &CoreContext,
        changesets: impl IntoIterator<Item = HgChangesetId>,
        bookmark: &BookmarkName,
        maybe_pushvars: Option<&HashMap<String, Bytes>>,
    ) -> Result<Vec<HookOutcome>, Error> {
        debug!(ctx.logger(), "Running hooks for bookmark {:?}", bookmark);

        let cs_hooks = self.changeset_hooks_for_bookmark(bookmark);
        let file_hooks = self.file_hooks_for_bookmark(bookmark);

        let cs_futs = FuturesUnordered::new();
        let file_futs = FuturesUnordered::new();

        for cs_id in changesets {
            cs_futs.push(self.run_changeset_hooks_for_changeset_id(
                ctx,
                cs_id.clone(),
                &cs_hooks,
                maybe_pushvars,
                bookmark,
            ));
            file_futs.push(self.run_file_hooks_for_changeset_id(
                ctx,
                cs_id,
                &file_hooks,
                maybe_pushvars,
                bookmark,
            ));
        }

        let (cs_hook_results, file_hook_results): (Vec<_>, Vec<_>) =
            try_join(cs_futs.try_collect(), file_futs.try_collect()).await?;
        Ok(cs_hook_results
            .into_iter()
            .flat_map(|r| r.into_iter())
            .chain(file_hook_results.into_iter().flat_map(|r| r.into_iter()))
            .collect())
    }

    // Changeset hooks

    async fn run_changeset_hooks_for_changeset_id(
        &self,
        ctx: &CoreContext,
        changeset_id: HgChangesetId,
        hooks: &Vec<String>,
        maybe_pushvars: Option<&HashMap<String, Bytes>>,
        bookmark: &BookmarkName,
    ) -> Result<Vec<HookOutcome>, Error> {
        debug!(
            ctx.logger(),
            "Running changeset hooks for changeset id {:?}", changeset_id
        );

        let hooks: Vec<_> = hooks
            .iter()
            .map(|hook_name| {
                let hook = self
                    .changeset_hooks
                    .get(hook_name)
                    .ok_or(ErrorKind::NoSuchHook(hook_name.to_string()))?;
                Ok((hook_name.clone(), hook.clone()))
            })
            .collect::<Result<_, Error>>()?;
        cloned!(mut self.scuba);
        scuba.add("hash", changeset_id.to_hex().to_string());

        let hcs = self.get_hook_changeset(&ctx, changeset_id).await?;
        let hooks = HookManager::filter_bypassed_hooks(hooks, &hcs.comments, maybe_pushvars);

        let res = HookManager::run_changeset_hooks_for_changeset(ctx, hcs, hooks, bookmark, scuba)
            .await?;
        Ok(res
            .into_iter()
            .map(|(hook_name, exec)| {
                HookOutcome::ChangesetHook(
                    ChangesetHookExecutionID {
                        cs_id: changeset_id,
                        hook_name,
                    },
                    exec,
                )
            })
            .collect())
    }

    async fn run_changeset_hooks_for_changeset<'book, 'ctx: 'book>(
        ctx: &'ctx CoreContext,
        changeset: HookChangeset,
        hooks: Vec<(String, Arc<dyn Hook<HookChangeset>>, HookConfig)>,
        bookmark: &'book BookmarkName,
        scuba: ScubaSampleBuilder,
    ) -> Result<Vec<(String, HookExecution)>, Error> {
        try_join_all(hooks.into_iter().map(|(hook_name, hook, config)| {
            HookManager::run_hook(
                ctx,
                hook,
                HookContext::new(hook_name, config, changeset.clone(), bookmark),
                scuba.clone(),
            )
        }))
        .await
    }

    // File hooks

    async fn run_file_hooks_for_changeset_id(
        &self,
        ctx: &CoreContext,
        changeset_id: HgChangesetId,
        hooks: &Vec<String>,
        maybe_pushvars: Option<&HashMap<String, Bytes>>,
        bookmark: &BookmarkName,
    ) -> Result<Vec<HookOutcome>, Error> {
        debug!(
            ctx.logger(),
            "Running file hooks for changeset id {:?}", changeset_id
        );

        let hooks: Vec<_> = hooks
            .iter()
            .map(|hook_name| {
                let hook = self
                    .file_hooks
                    .get(hook_name)
                    .ok_or(ErrorKind::NoSuchHook(hook_name.to_string()))?;
                Ok((hook_name.clone(), hook.clone()))
            })
            .collect::<Result<_, Error>>()?;
        cloned!(mut self.scuba);
        scuba.add("hash", changeset_id.to_hex().to_string());

        let hcs = self.get_hook_changeset(ctx, changeset_id).await?;
        let hooks = HookManager::filter_bypassed_hooks(hooks, &hcs.comments, maybe_pushvars);

        HookManager::run_file_hooks_for_changeset(ctx, changeset_id, &hcs, hooks, bookmark, scuba)
            .await
    }

    fn run_file_hooks_for_changeset<'cs, 'book: 'cs, 'ctx: 'cs>(
        ctx: &'ctx CoreContext,
        changeset_id: HgChangesetId,
        changeset: &'cs HookChangeset,
        hooks: Vec<(String, Arc<dyn Hook<HookFile>>, HookConfig)>,
        bookmark: &'book BookmarkName,
        scuba: ScubaSampleBuilder,
    ) -> impl Future<Output = Result<Vec<HookOutcome>, Error>> + 'cs {
        let v: Vec<_> = changeset
            .files
            .iter()
            // Do not run file hooks for deleted files
            .filter_map(move |file| {
                match file.ty {
                    ChangedFileType::Added | ChangedFileType::Modified => Some(
                        HookManager::run_file_hooks(
                            ctx,
                            changeset_id,
                            file.clone(),
                            hooks.clone(),
                            bookmark,
                            scuba.clone(),
                        )
                    ),
                    ChangedFileType::Deleted => None,
                }
            })
            .collect();
        try_join_all(v).map_ok(|vv| vv.into_iter().flatten().collect())
    }

    async fn run_file_hooks<'book, 'ctx: 'book>(
        ctx: &'ctx CoreContext,
        cs_id: HgChangesetId,
        file: HookFile,
        hooks: Vec<(String, Arc<dyn Hook<HookFile>>, HookConfig)>,
        bookmark: &'book BookmarkName,
        scuba: ScubaSampleBuilder,
    ) -> Result<Vec<HookOutcome>, Error> {
        let hook_futs = hooks.into_iter().map(move |(hook_name, hook, config)| {
            let hook_context =
                HookContext::new(hook_name.to_string(), config, file.clone(), bookmark);

            cloned!(mut scuba);
            scuba.add("hash", cs_id.to_hex().to_string());

            HookManager::run_hook(ctx, hook, hook_context, scuba).map_ok({
                cloned!(file, bookmark);
                move |(hook_name, exec)| {
                    HookOutcome::FileHook(
                        FileHookExecutionID {
                            cs_id,
                            hook_name,
                            file,
                            bookmark,
                        },
                        exec,
                    )
                }
            })
        });
        try_join_all(hook_futs).await
    }

    async fn run_hook<T: Clone>(
        ctx: &CoreContext,
        hook: Arc<dyn Hook<T>>,
        hook_context: HookContext<T>,
        mut scuba: ScubaSampleBuilder,
    ) -> Result<(String, HookExecution), Error> {
        let hook_name = hook_context.hook_name.clone();
        debug!(ctx.logger(), "Running hook {:?}", hook_context.hook_name);

        // Try getting the source hostname, otherwise use the unix name.
        let user_option = ctx
            .source_hostname()
            .as_ref()
            .or(ctx.user_unix_name().as_ref())
            .map(|s| s.as_str());

        if let Some(user) = user_option {
            scuba.add("user", user);
        }

        scuba.add("hook", hook_name.clone());

        let (stats, result) = hook.run(ctx, hook_context).timed().await;

        if let Err(e) = result.as_ref() {
            scuba.add("stderr", e.to_string());
        }

        let elapsed = stats.completion_time.as_millis() as i64;

        scuba
            .add("elapsed", elapsed)
            .add("total_time", elapsed)
            .add("errorcode", result.is_err() as i32)
            .add("failed_hooks", result.is_err() as i32)
            .log();

        let he = result.map_err(|e| e.context(format!("while executing hook {}", hook_name)))?;
        Ok((hook_name, he))
    }

    async fn get_hook_changeset(
        &self,
        ctx: &CoreContext,
        changeset_id: HgChangesetId,
    ) -> Result<HookChangeset, Error> {
        let content_store = self.content_store.clone();
        let hg_changeset = self
            .changeset_store
            .get_changeset_by_changesetid(ctx, changeset_id);
        let changed_files = self.changeset_store.get_changed_files(ctx, changeset_id);
        let reviewers_acl_checker = self.reviewers_acl_checker.clone();

        let (changeset, changed_files) = try_join(hg_changeset, changed_files).await?;

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

#[async_trait]
pub trait Hook<T>: Send + Sync
where
    T: Clone,
{
    async fn run<'a, 'b: 'a>(
        &'a self,
        ctx: &'b CoreContext,
        hook_context: HookContext<T>,
    ) -> Result<HookExecution, Error>;
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

    pub async fn len(&self, ctx: &CoreContext) -> Result<u64, Error> {
        let path = MPath::new(self.path.as_bytes())?;
        match self.hash_and_type {
            Some((entry_id, _)) => self.content_store.get_file_size(ctx, entry_id).await,
            None => Err(ErrorKind::MissingFile(self.changeset_id, path.into()).into()),
        }
    }

    pub async fn file_text(&self, ctx: &CoreContext) -> Result<Option<FileBytes>, Error> {
        let path = MPath::new(self.path.as_bytes())?;
        match self.hash_and_type {
            Some((id, _)) => self.content_store.get_file_text(ctx, id).await,
            None => Err(ErrorKind::MissingFile(self.changeset_id, path.into()).into()),
        }
    }

    pub fn file_type(&self, _ctx: &CoreContext) -> Result<FileType, Error> {
        let path = MPath::new(self.path.as_bytes())?;

        self.hash_and_type
            .ok_or(ErrorKind::MissingFile(self.changeset_id, path.into()).into())
            .map(|(_, file_type)| file_type)
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

    pub async fn file_text(
        &self,
        ctx: &CoreContext,
        path: String,
    ) -> Result<Option<FileBytes>, Error> {
        let path = MPath::new(path.as_bytes())?;
        let id = self
            .content_store
            .resolve_path(ctx, self.changeset_id, path)
            .await?;
        match id {
            Some(id) => self.content_store.get_file_text(ctx, id).await,
            None => Ok(None),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum HookOutcome {
    ChangesetHook(ChangesetHookExecutionID, HookExecution),
    FileHook(FileHookExecutionID, HookExecution),
}

impl fmt::Display for HookOutcome {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            HookOutcome::ChangesetHook(id, exec) => {
                write!(f, "{} for {}: {}", id.hook_name, id.cs_id, exec)
            }
            HookOutcome::FileHook(id, exec) => write!(
                f,
                "{} for {} file {}: {}",
                id.hook_name, id.cs_id, id.file.path, exec
            ),
        }
    }
}

impl HookOutcome {
    pub fn is_rejection(&self) -> bool {
        let exec = match self {
            HookOutcome::ChangesetHook(_, exec) => exec,
            HookOutcome::FileHook(_, exec) => exec,
        };
        match exec {
            HookExecution::Accepted => false,
            HookExecution::Rejected(_) => true,
        }
    }

    pub fn get_hook_name(&self) -> &str {
        match self {
            HookOutcome::ChangesetHook(id, _) => &id.hook_name,
            HookOutcome::FileHook(id, _) => &id.hook_name,
        }
    }

    pub fn get_file_path(&self) -> Option<&str> {
        match self {
            HookOutcome::ChangesetHook(..) => None,
            HookOutcome::FileHook(id, _) => Some(&id.file.path),
        }
    }

    pub fn get_cs_id(&self) -> HgChangesetId {
        match self {
            HookOutcome::ChangesetHook(id, _) => id.cs_id,
            HookOutcome::FileHook(id, _) => id.cs_id,
        }
    }

    pub fn get_execution(&self) -> &HookExecution {
        match self {
            HookOutcome::ChangesetHook(_, exec) => exec,
            HookOutcome::FileHook(_, exec) => exec,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum HookExecution {
    Accepted,
    Rejected(HookRejectionInfo),
}

impl From<HookOutcome> for HookExecution {
    fn from(outcome: HookOutcome) -> Self {
        match outcome {
            HookOutcome::ChangesetHook(_, r) => r,
            HookOutcome::FileHook(_, r) => r,
        }
    }
}

impl fmt::Display for HookExecution {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            HookExecution::Accepted => write!(f, "Accepted"),
            HookExecution::Rejected(reason) => write!(f, "Rejected: {}", reason.long_description),
        }
    }
}

/// Information on why the hook rejected the changeset
#[derive(Clone, Debug, PartialEq)]
pub struct HookRejectionInfo {
    /// A short description for summarizing this failure with similar failures
    pub description: &'static str,
    /// A full explanation of what went wrong, suitable for presenting to the user (should include guidance for fixing this failure, where possible)
    pub long_description: String,
}

impl HookRejectionInfo {
    /// A rejection with just a short description
    /// The text should just summarize this failure - it should not be different on different invocations of this hook
    pub fn new(description: &'static str) -> Self {
        Self::new_long(description, description.to_string())
    }

    /// A rejection with a possible per-invocation fix explanation.
    pub fn new_long<OS>(description: &'static str, long_description: OS) -> Self
    where
        OS: Into<Option<String>>,
    {
        let long_description = long_description
            .into()
            .unwrap_or_else(|| description.to_string());
        Self {
            description,
            long_description,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Hash, Eq)]
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
        bookmark: &BookmarkName,
    ) -> HookContext<T> {
        HookContext {
            hook_name,
            config,
            data,
            bookmark: bookmark.clone(),
        }
    }
}
