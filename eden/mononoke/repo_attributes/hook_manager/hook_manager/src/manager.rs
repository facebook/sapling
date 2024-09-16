/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::str;
use std::sync::Arc;

use anyhow::Error;
use anyhow::Result;
use bookmarks_types::BookmarkKey;
use bytes::Bytes;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::stream::futures_unordered::FuturesUnordered;
use futures::stream::TryStreamExt;
use futures::try_join;
use futures::Future;
use futures::TryFutureExt;
use futures_stats::TimedFutureExt;
use metaconfig_types::BookmarkOrRegex;
use metaconfig_types::HookBypass;
use metaconfig_types::HookConfig;
use metaconfig_types::HookManagerParams;
use mononoke_types::BasicFileChange;
use mononoke_types::BonsaiChangeset;
use mononoke_types::NonRootMPath;
use permission_checker::AclProvider;
use permission_checker::ArcMembershipChecker;
use permission_checker::NeverMember;
use regex::Regex;
use repo_permission_checker::ArcRepoPermissionChecker;
use repo_permission_checker::NeverAllowRepoPermissionChecker;
use scuba::builder::ServerData;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::debug;

use crate::errors::HookManagerError;
use crate::provider::HookStateProvider;
use crate::BookmarkHook;
use crate::BookmarkHookExecutionId;
use crate::ChangesetHook;
use crate::ChangesetHookExecutionId;
use crate::CrossRepoPushSource;
use crate::FileHook;
use crate::FileHookExecutionId;
use crate::HookExecution;
use crate::HookOutcome;
use crate::PushAuthoredBy;

/// Manages hooks and allows them to be installed and uninstalled given a name
/// Knows how to run hooks

#[facet::facet]
pub struct HookManager {
    repo_name: String,
    hooks: HashMap<String, Hook>,
    bookmark_hooks: HashMap<BookmarkKey, Vec<String>>,
    regex_hooks: Vec<(Regex, Vec<String>)>,
    content_provider: Box<dyn HookStateProvider>,
    reviewers_membership: ArcMembershipChecker,
    admin_membership: ArcMembershipChecker,
    repo_permission_checker: ArcRepoPermissionChecker,
    scuba: MononokeScubaSampleBuilder,
    all_hooks_bypassed: bool,
    scuba_bypassed_commits: MononokeScubaSampleBuilder,
}

impl HookManager {
    pub async fn new(
        fb: FacebookInit,
        acl_provider: &dyn AclProvider,
        content_provider: Box<dyn HookStateProvider>,
        hook_manager_params: HookManagerParams,
        repo_permission_checker: ArcRepoPermissionChecker,
        mut scuba: MononokeScubaSampleBuilder,
        repo_name: String,
    ) -> Result<HookManager> {
        let hooks = HashMap::new();

        scuba
            .add("driver", "mononoke")
            .add("scm", "hg")
            .add_mapped_common_server_data(|data| match data {
                ServerData::Hostname => "host",
                _ => data.default_key(),
            });

        let (reviewers_membership, admin_membership, repo_permission_checker) =
            if hook_manager_params.disable_acl_checker {
                (
                    NeverMember::new(),
                    NeverMember::new(),
                    Arc::new(NeverAllowRepoPermissionChecker {}) as ArcRepoPermissionChecker,
                )
            } else {
                let (reviewers_membership, admin_membership) =
                    try_join!(acl_provider.reviewers_group(), acl_provider.admin_group(),)?;
                (
                    reviewers_membership,
                    admin_membership,
                    repo_permission_checker,
                )
            };

        let scuba_bypassed_commits: MononokeScubaSampleBuilder =
            scuba_ext::MononokeScubaSampleBuilder::with_opt_table(
                fb,
                hook_manager_params.bypassed_commits_scuba_table,
            )?;

        Ok(HookManager {
            repo_name,
            hooks,
            bookmark_hooks: HashMap::new(),
            regex_hooks: Vec::new(),
            content_provider,
            reviewers_membership: reviewers_membership.into(),
            admin_membership: admin_membership.into(),
            scuba,
            all_hooks_bypassed: hook_manager_params.all_hooks_bypassed,
            scuba_bypassed_commits,
            repo_permission_checker,
        })
    }

    // Create a very simple HookManager, for use inside of the TestRepoFactory.
    pub fn new_test(repo_name: String, content_provider: Box<dyn HookStateProvider>) -> Self {
        Self {
            repo_name,
            hooks: HashMap::new(),
            bookmark_hooks: HashMap::new(),
            regex_hooks: Vec::new(),
            content_provider,
            reviewers_membership: NeverMember::new().into(),
            admin_membership: NeverMember::new().into(),
            scuba: MononokeScubaSampleBuilder::with_discard(),
            all_hooks_bypassed: false,
            scuba_bypassed_commits: MononokeScubaSampleBuilder::with_discard(),
            repo_permission_checker: Arc::new(NeverAllowRepoPermissionChecker {}),
        }
    }

    pub fn register_bookmark_hook(
        &mut self,
        hook_name: &str,
        hook: Box<dyn BookmarkHook>,
        config: HookConfig,
    ) {
        self.hooks
            .insert(hook_name.to_string(), Hook::from_bookmark(hook, config));
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
                self.regex_hooks.push((regex.into_inner(), hooks));
            }
        }
    }

    pub fn get_reviewers_perm_checker(&self) -> ArcMembershipChecker {
        self.reviewers_membership.clone()
    }

    pub fn get_admin_perm_checker(&self) -> ArcMembershipChecker {
        self.admin_membership.clone()
    }

    pub fn get_repo_perm_checker(&self) -> ArcRepoPermissionChecker {
        self.repo_permission_checker.clone()
    }

    pub fn hooks_exist_for_bookmark(&self, bookmark: &BookmarkKey) -> bool {
        if self.bookmark_hooks.contains_key(bookmark) {
            return true;
        }

        let bookmark = bookmark.as_str();
        self.regex_hooks
            .iter()
            .any(|(regex, _)| regex.is_match(bookmark))
    }

    pub fn repo_name(&self) -> &String {
        &self.repo_name
    }

    fn hooks_for_bookmark<'a>(
        &'a self,
        bookmark: &BookmarkKey,
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

    pub fn all_hooks_bypassed(&self) -> bool {
        self.all_hooks_bypassed
    }

    pub fn scuba_bypassed_commits(&self) -> &MononokeScubaSampleBuilder {
        &self.scuba_bypassed_commits
    }

    pub async fn run_bookmark_hooks_for_bookmark(
        &self,
        ctx: &CoreContext,
        to: &BonsaiChangeset,
        bookmark: &BookmarkKey,
        maybe_pushvars: Option<&HashMap<String, Bytes>>,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
    ) -> Result<Vec<HookOutcome>, Error> {
        debug!(
            ctx.logger(),
            "Running bookmark hooks for bookmark {:?}", bookmark
        );

        let hooks = self.hooks_for_bookmark(bookmark);

        let futs = FuturesUnordered::new();

        let mut scuba = self.scuba.clone();
        let username = ctx.metadata().unix_name();
        let user_option = ctx.metadata().client_hostname().or(username);

        if let Some(user) = user_option {
            scuba.add("user", user);
        }

        for hook_name in hooks {
            let hook = self
                .hooks
                .get(hook_name)
                .ok_or_else(|| HookManagerError::NoSuchHook(hook_name.to_string()))?;

            let mut scuba = scuba.clone();
            scuba.add("hook", hook_name.to_string());
            scuba.add("to", to.get_changeset_id().to_string());

            if let Some(bypass_reason) = get_bypass_reason(
                hook.get_config().bypass.as_ref(),
                to.message(),
                maybe_pushvars,
            ) {
                scuba.add("bypass_reason", bypass_reason);
                scuba.log();
                continue;
            }

            for future in hook.get_futures_for_bookmark_hooks(
                ctx,
                bookmark,
                &*self.content_provider,
                hook_name,
                to,
                scuba,
                cross_repo_push_source,
                push_authored_by,
                hook.get_config().log_only,
            ) {
                futs.push(future);
            }
        }
        futs.try_collect().await
    }

    pub async fn run_changesets_hooks_for_bookmark(
        &self,
        ctx: &CoreContext,
        changesets: impl Clone + itertools::Itertools<Item = &BonsaiChangeset>,
        bookmark: &BookmarkKey,
        maybe_pushvars: Option<&HashMap<String, Bytes>>,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
    ) -> Result<Vec<HookOutcome>, Error> {
        debug!(ctx.logger(), "Running hooks for bookmark {:?}", bookmark);

        let hooks = self.hooks_for_bookmark(bookmark);

        let futs = FuturesUnordered::new();

        let mut scuba = self.scuba.clone();
        let username = ctx.metadata().unix_name();
        let user_option = ctx.metadata().client_hostname().or(username);

        if let Some(user) = user_option {
            scuba.add("user", user);
        }

        for (cs, hook_name) in changesets.cartesian_product(hooks) {
            let hook = self
                .hooks
                .get(hook_name)
                .ok_or_else(|| HookManagerError::NoSuchHook(hook_name.to_string()))?;

            let mut scuba = scuba.clone();
            scuba.add("hook", hook_name.to_string());
            scuba.add("hash", cs.get_changeset_id().to_string());

            if let Some(bypass_reason) = get_bypass_reason(
                hook.get_config().bypass.as_ref(),
                cs.message(),
                maybe_pushvars,
            ) {
                scuba.add("bypass_reason", bypass_reason);
                scuba.log();
                continue;
            }

            for future in hook.get_futures_for_changeset_or_file_hooks(
                ctx,
                bookmark,
                &*self.content_provider,
                hook_name,
                cs,
                scuba,
                cross_repo_push_source,
                push_authored_by,
                hook.get_config().log_only,
            ) {
                futs.push(future);
            }
        }
        futs.try_collect().await
    }
}

fn get_bypass_reason(
    bypass: Option<&HookBypass>,
    cs_msg: &str,
    maybe_pushvars: Option<&HashMap<String, Bytes>>,
) -> Option<String> {
    let bypass = bypass?;

    if let Some(bypass_string) = bypass.commit_message_bypass() {
        if cs_msg.contains(bypass_string) {
            return Some(format!("bypass string: {}", bypass_string));
        }
    }

    if let Some((name, value)) = bypass.pushvar_bypass() {
        if let Some(pushvars) = maybe_pushvars {
            let pushvar_val = pushvars
                .get(name)
                .map(|bytes| String::from_utf8(bytes.to_vec()));

            if let Some(Ok(pushvar_val)) = pushvar_val {
                if pushvar_val == *value {
                    return Some(format!("bypass pushvar: {}={}", name, value));
                }
            }
        }
    }

    None
}

enum Hook {
    Bookmark(Box<dyn BookmarkHook>, HookConfig),
    Changeset(Box<dyn ChangesetHook>, HookConfig),
    File(Box<dyn FileHook>, HookConfig),
}

enum HookInstance<'a> {
    Bookmark(&'a dyn BookmarkHook),
    Changeset(&'a dyn ChangesetHook),
    File(
        &'a dyn FileHook,
        &'a NonRootMPath,
        Option<&'a BasicFileChange>,
    ),
}

impl<'a> HookInstance<'a> {
    async fn run_on_bookmark(
        self,
        ctx: &CoreContext,
        bookmark: &BookmarkKey,
        content_provider: &dyn HookStateProvider,
        hook_name: &str,
        mut scuba: MononokeScubaSampleBuilder,
        to: &BonsaiChangeset,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
        log_only: bool,
    ) -> Result<HookOutcome, Error> {
        let cs_id = to.get_changeset_id();
        let (stats, mut result) = match self {
            Self::Bookmark(hook) => {
                hook.run(
                    ctx,
                    bookmark,
                    to,
                    content_provider,
                    cross_repo_push_source,
                    push_authored_by,
                )
                .map_ok(|exec| {
                    HookOutcome::BookmarkHook(
                        BookmarkHookExecutionId {
                            cs_id,
                            bookmark_name: bookmark.to_string(),
                            hook_name: hook_name.to_string(),
                        },
                        exec,
                    )
                })
                .timed()
                .await
            }
            Self::Changeset(..) => {
                // Don't run changeset hook on bookmark. Just accept the change.
                async { Ok(HookExecution::Accepted) }
                    .map_ok(|exec| {
                        HookOutcome::ChangesetHook(
                            ChangesetHookExecutionId {
                                cs_id,
                                hook_name: hook_name.to_string(),
                            },
                            exec,
                        )
                    })
                    .timed()
                    .await
            }
            Self::File(_, path, _) => {
                // Don't run file hook on bookmark. Just accept the change.
                async { Ok(HookExecution::Accepted) }
                    .map_ok(|exec| {
                        HookOutcome::FileHook(
                            FileHookExecutionId {
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

        let mut errorcode = 0;
        let mut failed_hooks = 0;
        let mut stderr = None;

        match result.as_mut() {
            Ok(outcome) => match outcome.get_execution() {
                HookExecution::Accepted => {
                    // Nothing to do
                }
                HookExecution::Rejected(info) if log_only => {
                    scuba.add("log_only_rejection", info.long_description.clone());
                    // Convert to accepted as we are only logging.
                    outcome.set_execution(HookExecution::Accepted);
                }
                HookExecution::Rejected(info) => {
                    failed_hooks = 1;
                    errorcode = 1;
                    stderr = Some(info.long_description.clone());
                }
            },
            Err(e) => {
                errorcode = 1;
                stderr = Some(format!("{:?}", e));
                scuba.add("internal_failure", true);
            }
        };

        if let Some(stderr) = stderr {
            scuba.add("stderr", stderr);
        }

        let elapsed = stats.completion_time.as_millis() as i64;
        scuba
            .add("elapsed", elapsed)
            .add("total_time", elapsed)
            .add("errorcode", errorcode)
            .add("failed_hooks", failed_hooks)
            .log();

        result.map_err(|e| e.context(format!("while executing hook {}", hook_name)))
    }

    async fn run_on_changeset(
        self,
        ctx: &CoreContext,
        bookmark: &BookmarkKey,
        content_provider: &dyn HookStateProvider,
        hook_name: &str,
        mut scuba: MononokeScubaSampleBuilder,
        cs: &BonsaiChangeset,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
        log_only: bool,
    ) -> Result<HookOutcome, Error> {
        let cs_id = cs.get_changeset_id();
        let (stats, mut result) = match self {
            Self::Bookmark(..) => {
                // Don't run bookmark hook on changeset. Just accept the change.
                async { Ok(HookExecution::Accepted) }
                    .map_ok(|exec| {
                        HookOutcome::ChangesetHook(
                            ChangesetHookExecutionId {
                                cs_id,
                                hook_name: hook_name.to_string(),
                            },
                            exec,
                        )
                    })
                    .timed()
                    .await
            }
            Self::Changeset(hook) => {
                hook.run(
                    ctx,
                    bookmark,
                    cs,
                    content_provider,
                    cross_repo_push_source,
                    push_authored_by,
                )
                .map_ok(|exec| {
                    HookOutcome::ChangesetHook(
                        ChangesetHookExecutionId {
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
                hook.run(
                    ctx,
                    content_provider,
                    change,
                    path,
                    cross_repo_push_source,
                    push_authored_by,
                )
                .map_ok(|exec| {
                    HookOutcome::FileHook(
                        FileHookExecutionId {
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

        let mut errorcode = 0;
        let mut failed_hooks = 0;
        let mut stderr = None;

        match result.as_mut() {
            Ok(outcome) => match outcome.get_execution() {
                HookExecution::Accepted => {
                    // Nothing to do
                }
                HookExecution::Rejected(info) if log_only => {
                    scuba.add("log_only_rejection", info.long_description.clone());
                    // Convert to accepted as we are only logging.
                    outcome.set_execution(HookExecution::Accepted);
                }
                HookExecution::Rejected(info) => {
                    failed_hooks = 1;
                    stderr = Some(info.long_description.clone());
                }
            },
            Err(e) => {
                errorcode = 1;
                stderr = Some(format!("{:?}", e));
            }
        };

        if let Some(stderr) = stderr {
            scuba.add("stderr", stderr);
        }

        let elapsed = stats.completion_time.as_millis() as i64;
        scuba
            .add("elapsed", elapsed)
            .add("total_time", elapsed)
            .add("errorcode", errorcode)
            .add("failed_hooks", failed_hooks)
            .log();

        result.map_err(|e| e.context(format!("while executing hook {}", hook_name)))
    }
}

impl Hook {
    pub fn from_bookmark(hook: Box<dyn BookmarkHook>, config: HookConfig) -> Self {
        Self::Bookmark(hook, config)
    }

    pub fn from_changeset(hook: Box<dyn ChangesetHook>, config: HookConfig) -> Self {
        Self::Changeset(hook, config)
    }

    pub fn from_file(hook: Box<dyn FileHook>, config: HookConfig) -> Self {
        Self::File(hook, config)
    }

    pub fn get_config(&self) -> &HookConfig {
        match self {
            Self::Bookmark(_, config) => config,
            Self::Changeset(_, config) => config,
            Self::File(_, config) => config,
        }
    }

    pub fn get_futures_for_bookmark_hooks<'a: 'cs, 'cs>(
        &'a self,
        ctx: &'a CoreContext,
        bookmark: &'a BookmarkKey,
        content_provider: &'a dyn HookStateProvider,
        hook_name: &'cs str,
        to: &'cs BonsaiChangeset,
        scuba: MononokeScubaSampleBuilder,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
        log_only: bool,
    ) -> impl Iterator<Item = impl Future<Output = Result<HookOutcome, Error>> + 'cs> + 'cs {
        let mut futures = Vec::new();

        match self {
            Self::Bookmark(hook, _) => {
                futures.push(HookInstance::Bookmark(&**hook).run_on_bookmark(
                    ctx,
                    bookmark,
                    content_provider,
                    hook_name,
                    scuba,
                    to,
                    cross_repo_push_source,
                    push_authored_by,
                    log_only,
                ))
            }
            Self::Changeset(..) | Self::File(..) =>
                /* Not a bookmark hook */
                {}
        };
        futures.into_iter()
    }

    pub fn get_futures_for_changeset_or_file_hooks<'a: 'cs, 'cs>(
        &'a self,
        ctx: &'a CoreContext,
        bookmark: &'a BookmarkKey,
        content_provider: &'a dyn HookStateProvider,
        hook_name: &'cs str,
        cs: &'cs BonsaiChangeset,
        scuba: MononokeScubaSampleBuilder,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
        log_only: bool,
    ) -> impl Iterator<Item = impl Future<Output = Result<HookOutcome, Error>> + 'cs> + 'cs {
        let mut futures = Vec::new();

        match self {
            Self::Changeset(hook, _) => {
                futures.push(HookInstance::Changeset(&**hook).run_on_changeset(
                    ctx,
                    bookmark,
                    content_provider,
                    hook_name,
                    scuba,
                    cs,
                    cross_repo_push_source,
                    push_authored_by,
                    log_only,
                ))
            }
            Self::File(hook, _) => {
                futures.extend(cs.simplified_file_changes().map(move |(path, change)| {
                    HookInstance::File(&**hook, path, change).run_on_changeset(
                        ctx,
                        bookmark,
                        content_provider,
                        hook_name,
                        scuba.clone(),
                        cs,
                        cross_repo_push_source,
                        push_authored_by,
                        log_only,
                    )
                }))
            }
            Self::Bookmark(..) =>
                /* Not a changeset or file hook */
                {}
        };
        futures.into_iter()
    }
}

#[cfg(test)]
mod test {
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn test_commit_message_bypass() {
        let bypass = HookBypass::new_with_commit_msg("@mybypass".into());

        let r = get_bypass_reason(Some(&bypass), "@notbypass", None);
        assert!(r.is_none());

        let r = get_bypass_reason(Some(&bypass), "foo @mybypass bar", None);
        assert!(r.is_some());
    }

    #[mononoke::test]
    fn test_pushvar_bypass() {
        let bypass = HookBypass::new_with_pushvar("myvar".into(), "myvalue".into());

        let mut m = HashMap::new();
        let r = get_bypass_reason(Some(&bypass), "", Some(&m));
        assert!(r.is_none()); // No var

        m.insert("somevar".into(), "somevalue".as_bytes().into());
        let r = get_bypass_reason(Some(&bypass), "", Some(&m));
        assert!(r.is_none()); // wrong var

        m.insert("myvar".into(), "somevalue".as_bytes().into());
        let r = get_bypass_reason(Some(&bypass), "", Some(&m));
        assert!(r.is_none()); // wrong value

        m.insert("myvar".into(), "myvalue foo".as_bytes().into());
        let r = get_bypass_reason(Some(&bypass), "", Some(&m));
        assert!(r.is_none()); // wrong value

        m.insert("myvar".into(), "myvalue".as_bytes().into());
        let r = get_bypass_reason(Some(&bypass), "", Some(&m));
        assert!(r.is_some());
    }
}
