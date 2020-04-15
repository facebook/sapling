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

use aclchecker::{AclChecker, Identity};
use anyhow::{bail, Error};
use async_trait::async_trait;
use bookmarks::BookmarkName;
use bytes::Bytes;
use context::CoreContext;
pub use errors::*;
use fbinit::FacebookInit;
use futures::{
    stream::{futures_unordered::FuturesUnordered, TryStreamExt},
    Future, TryFutureExt,
};
use futures_stats::TimedFutureExt;
use hooks_content_stores::FileContentFetcher;
use metaconfig_types::{BookmarkOrRegex, HookBypass, HookConfig, HookManagerParams};
use mononoke_types::{BonsaiChangeset, ChangesetId, FileChange, MPath};
use regex::Regex;
use scuba::builder::ServerData;
use scuba_ext::ScubaSampleBuilder;
use slog::debug;
use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;
use std::str;
use std::sync::Arc;

/// Manages hooks and allows them to be installed and uninstalled given a name
/// Knows how to run hooks

pub struct HookManager {
    hooks: HashMap<String, Hook>,
    bookmark_hooks: HashMap<BookmarkName, Vec<String>>,
    regex_hooks: Vec<(Regex, Vec<String>)>,
    content_fetcher: Box<dyn FileContentFetcher>,
    reviewers_acl_checker: Arc<Option<AclChecker>>,
    scuba: ScubaSampleBuilder,
}

impl HookManager {
    pub fn new(
        fb: FacebookInit,
        content_fetcher: Box<dyn FileContentFetcher>,
        hook_manager_params: HookManagerParams,
        mut scuba: ScubaSampleBuilder,
    ) -> HookManager {
        let hooks = HashMap::new();

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
            hooks,
            bookmark_hooks: HashMap::new(),
            regex_hooks: Vec::new(),
            content_fetcher,
            reviewers_acl_checker: Arc::new(reviewers_acl_checker),
            scuba,
        }
    }

    pub fn register_changeset_hook(
        &mut self,
        hook_name: &str,
        hook: Box<dyn ChangesetHook>,
        config: HookConfig,
    ) {
        self.hooks
            .insert(hook_name.to_string(), Hook::from_changeset(hook, config));
    }

    pub fn register_file_hook(
        &mut self,
        hook_name: &str,
        hook: Box<dyn FileHook>,
        config: HookConfig,
    ) {
        self.hooks
            .insert(hook_name.to_string(), Hook::from_file(hook, config));
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

    pub(crate) fn get_reviewers_acl_checker(&self) -> Arc<Option<AclChecker>> {
        self.reviewers_acl_checker.clone()
    }

    fn hooks_for_bookmark<'a>(
        &'a self,
        bookmark: &BookmarkName,
    ) -> impl Iterator<Item = &'a str> + Clone {
        let mut hooks: Vec<&'a str> = match self.bookmark_hooks.get(bookmark) {
            Some(hooks) => hooks.iter().map(|a| a.as_str()).collect(),
            None => Vec::new(),
        };

        let bookmark_str = bookmark.to_string();
        for (regex, r_hooks) in &self.regex_hooks {
            if regex.is_match(&bookmark_str) {
                hooks.extend(r_hooks.iter().map(|a| a.as_str()));
            }
        }

        hooks.into_iter()
    }

    pub async fn run_hooks_for_bookmark(
        &self,
        ctx: &CoreContext,
        changesets: impl Iterator<Item = &BonsaiChangeset> + Clone + itertools::Itertools,
        bookmark: &BookmarkName,
        maybe_pushvars: Option<&HashMap<String, Bytes>>,
    ) -> Result<Vec<HookOutcome>, Error> {
        debug!(ctx.logger(), "Running hooks for bookmark {:?}", bookmark);

        let hooks = self.hooks_for_bookmark(bookmark);

        let futs = FuturesUnordered::new();

        let mut scuba = self.scuba.clone();
        let user_option = ctx
            .source_hostname()
            .as_ref()
            .or_else(|| ctx.user_unix_name().as_ref())
            .map(|s| s.as_str());

        if let Some(user) = user_option {
            scuba.add("user", user);
        }

        for (cs, hook_name) in changesets.cartesian_product(hooks) {
            let hook = self
                .hooks
                .get(hook_name)
                .ok_or_else(|| ErrorKind::NoSuchHook(hook_name.to_string()))?;
            if is_hook_bypassed(
                hook.get_config().bypass.as_ref(),
                cs.message(),
                maybe_pushvars,
            ) {
                continue;
            }

            let mut scuba = scuba.clone();
            scuba.add("hook", hook_name.to_string());

            for future in
                hook.get_futures(ctx, bookmark, &*self.content_fetcher, hook_name, cs, scuba)
            {
                futs.push(future);
            }
        }
        futs.try_collect().await
    }
}

fn is_hook_bypassed(
    bypass: Option<&HookBypass>,
    cs_msg: &str,
    maybe_pushvars: Option<&HashMap<String, Bytes>>,
) -> bool {
    bypass.map_or(false, move |bypass| match bypass {
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
            false
        }
    })
}

enum Hook {
    Changeset(Box<dyn ChangesetHook>, HookConfig),
    File(Box<dyn FileHook>, HookConfig),
}

enum HookInstance<'a> {
    Changeset(&'a dyn ChangesetHook),
    File(&'a dyn FileHook, &'a MPath, Option<&'a FileChange>),
}

impl<'a> HookInstance<'a> {
    async fn run(
        self,
        ctx: &CoreContext,
        bookmark: &BookmarkName,
        content_fetcher: &dyn FileContentFetcher,
        hook_name: &str,
        mut scuba: ScubaSampleBuilder,
        cs: &BonsaiChangeset,
        cs_id: ChangesetId,
    ) -> Result<HookOutcome, Error> {
        let (stats, result) = match self {
            Self::Changeset(hook) => {
                hook.run(ctx, bookmark, cs, content_fetcher)
                    .map_ok(|exec| {
                        HookOutcome::ChangesetHook(
                            ChangesetHookExecutionID {
                                cs_id,
                                hook_name: hook_name.to_string(),
                            },
                            exec,
                        )
                    })
                    .timed()
                    .await
            }
            Self::File(hook, path, change) => {
                hook.run(ctx, content_fetcher, change, path)
                    .map_ok(|exec| {
                        HookOutcome::FileHook(
                            FileHookExecutionID {
                                cs_id,
                                path: path.clone(),
                                hook_name: hook_name.to_string(),
                            },
                            exec,
                        )
                    })
                    .timed()
                    .await
            }
        };

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

        result.map_err(|e| e.context(format!("while executing hook {}", hook_name)))
    }
}

impl Hook {
    pub fn from_changeset(hook: Box<dyn ChangesetHook>, config: HookConfig) -> Self {
        Self::Changeset(hook, config)
    }

    pub fn from_file(hook: Box<dyn FileHook>, config: HookConfig) -> Self {
        Self::File(hook, config)
    }

    pub fn get_config(&self) -> &HookConfig {
        match self {
            Self::Changeset(_, config) => config,
            Self::File(_, config) => config,
        }
    }

    pub fn get_futures<'a: 'cs, 'cs>(
        &'a self,
        ctx: &'a CoreContext,
        bookmark: &'a BookmarkName,
        content_fetcher: &'a dyn FileContentFetcher,
        hook_name: &'cs str,
        cs: &'cs BonsaiChangeset,
        scuba: ScubaSampleBuilder,
    ) -> impl Iterator<Item = impl Future<Output = Result<HookOutcome, Error>> + 'cs> + 'cs {
        let mut futures = Vec::new();

        let cs_id = cs.get_changeset_id();

        match self {
            Self::Changeset(hook, _) => futures.push(HookInstance::Changeset(&**hook).run(
                ctx,
                bookmark,
                content_fetcher,
                &hook_name,
                scuba,
                cs,
                cs_id,
            )),
            Self::File(hook, _) => futures.extend(cs.file_changes().map(move |(path, change)| {
                HookInstance::File(&**hook, path, change).run(
                    ctx,
                    bookmark,
                    content_fetcher,
                    &hook_name,
                    scuba.clone(),
                    cs,
                    cs_id,
                )
            })),
        };
        futures.into_iter()
    }
}

#[async_trait]
pub trait ChangesetHook: Send + Sync {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'fetcher: 'cs>(
        &'this self,
        ctx: &'ctx CoreContext,
        bookmark: &BookmarkName,
        changeset: &'cs BonsaiChangeset,
        content_fetcher: &'fetcher dyn FileContentFetcher,
    ) -> Result<HookExecution, Error>;
}

#[async_trait]
pub trait FileHook: Send + Sync {
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'fetcher: 'change, 'path: 'change>(
        &'this self,
        ctx: &'ctx CoreContext,
        content_fetcher: &'fetcher dyn FileContentFetcher,
        change: Option<&'change FileChange>,
        path: &'path MPath,
    ) -> Result<HookExecution, Error>;
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
                id.hook_name, id.cs_id, id.path, exec
            ),
        }
    }
}

impl HookOutcome {
    pub fn is_rejection(&self) -> bool {
        match self.get_execution() {
            HookExecution::Accepted => false,
            HookExecution::Rejected(_) => true,
        }
    }

    pub fn is_accept(&self) -> bool {
        !self.is_rejection()
    }

    pub fn get_hook_name(&self) -> &str {
        match self {
            HookOutcome::ChangesetHook(id, _) => &id.hook_name,
            HookOutcome::FileHook(id, _) => &id.hook_name,
        }
    }

    pub fn get_file_path(&self) -> Option<&MPath> {
        match self {
            HookOutcome::ChangesetHook(..) => None,
            HookOutcome::FileHook(id, _) => Some(&id.path),
        }
    }

    pub fn get_changeset_id(&self) -> ChangesetId {
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
    pub cs_id: ChangesetId,
    pub hook_name: String,
    pub path: MPath,
}

#[derive(Clone, Debug, PartialEq, Hash, Eq)]
pub struct ChangesetHookExecutionID {
    pub cs_id: ChangesetId,
    pub hook_name: String,
}
