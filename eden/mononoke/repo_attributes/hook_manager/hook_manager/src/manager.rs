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
use cloned::cloned;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::future::BoxFuture;
use futures::stream::futures_unordered::FuturesUnordered;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::try_join;
use futures::Future;
use futures::FutureExt;
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
use crate::ChangesetHook;
use crate::CrossRepoPushSource;
use crate::FileHook;
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

            if let Some(bypass_reason) =
                get_bypassed_by_pushvar_reason(hook.get_config().bypass.as_ref(), maybe_pushvars)
            {
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
        changesets: &[BonsaiChangeset],
        bookmark: &BookmarkKey,
        maybe_pushvars: Option<&HashMap<String, Bytes>>,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
    ) -> Result<Vec<HookOutcome>, Error> {
        debug!(ctx.logger(), "Running hooks for bookmark {:?}", bookmark);

        let hooks = self.hooks_for_bookmark(bookmark);

        let mut scuba = self.scuba.clone();
        let username = ctx.metadata().unix_name();
        let user_option = ctx.metadata().client_hostname().or(username);

        if let Some(user) = user_option {
            scuba.add("user", user);
        }

        let (batched, individual) = hooks
            .map(|hook_name| {
                let hook = self
                    .hooks
                    .get(hook_name)
                    .ok_or_else(|| HookManagerError::NoSuchHook(hook_name.to_string()))?;
                Ok((hook_name, hook))
            })
            // Collapse out if an error happened
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            // Skip any hook that's entirely bypassed due to a pushvar
            .filter(|(hook_name, hook)| {
                log_if_bypassed_by_pushvar(&scuba, hook_name, hook, changesets, maybe_pushvars)
            })
            .map(|(hook_name, hook)| {
                // Skip any changeset that explicitly bypasses this hook in its commit message
                let changesets = changesets
                    .iter()
                    .filter(|cs| log_if_bypassed_by_commit_msg(&scuba, hook_name, hook, cs))
                    .collect::<Vec<_>>();
                cloned!(mut scuba);
                scuba.add("hook", hook_name.to_string());
                hook.get_futures_for_changeset_or_file_hooks(
                    ctx,
                    bookmark,
                    &*self.content_provider,
                    hook_name,
                    changesets,
                    scuba,
                    cross_repo_push_source,
                    push_authored_by,
                    hook.get_config().log_only,
                )
            })
            .partition::<Vec<_>, _>(HooksOutcome::is_batched);

        let individual_concurrency = justknobs::get_as::<usize>(
            "scm/mononoke:bookmark_movement_changeset_hooks_individual_concurency",
            Some(&self.repo_name),
        )
        .unwrap_or(100);
        let batched_concurrency = justknobs::get_as::<usize>(
            "scm/mononoke:bookmark_movement_changeset_hooks_batched_concurency",
            Some(&self.repo_name),
        )
        .unwrap_or(10);

        // Avoid mixing fast and slow futures by joining two streams:
        // * One that runs fast futures that operate on a single changeset or file.
        //   Such futures should be lightweight enough that we can run 100 of them concurrently
        // * One that runs slow futures that process multiple changesets at once.
        //   Such futures may take longer waiting for IO, so we only run 10 of them concurrently
        let individual_fut =
            futures::stream::iter(individual.into_iter().flat_map(HooksOutcome::into_inner))
                .boxed()
                .buffer_unordered(individual_concurrency)
                .try_collect::<Vec<_>>();
        let batched_fut =
            futures::stream::iter(batched.into_iter().flat_map(HooksOutcome::into_inner))
                .boxed()
                .buffer_unordered(batched_concurrency)
                .try_collect::<Vec<_>>();

        let (individual_res, batched_res) = futures::try_join!(individual_fut, batched_fut)?;
        Ok(individual_res
            .into_iter()
            .chain(batched_res.into_iter())
            .collect())
    }
}

fn log_if_bypassed_by_commit_msg(
    scuba: &MononokeScubaSampleBuilder,
    hook_name: &str,
    hook: &Hook,
    cs: &BonsaiChangeset,
) -> bool {
    if let Some(bypass_reason) =
        get_bypassed_by_commit_msg_reason(hook.get_config().bypass.as_ref(), cs.message())
    {
        log_bypassed_changeset(scuba, hook_name, cs, &bypass_reason);
        false
    } else {
        true
    }
}

fn log_if_bypassed_by_pushvar(
    scuba: &MononokeScubaSampleBuilder,
    hook_name: &str,
    hook: &Hook,
    changesets: &[BonsaiChangeset],
    maybe_pushvars: Option<&HashMap<String, Bytes>>,
) -> bool {
    if let Some(bypass_reason) =
        get_bypassed_by_pushvar_reason(hook.get_config().bypass.as_ref(), maybe_pushvars)
    {
        // Log all bypassed hooks. No need to be async
        for cs in changesets {
            log_bypassed_changeset(scuba, hook_name, cs, &bypass_reason);
        }
        false
    } else {
        true
    }
}

fn log_bypassed_changeset(
    scuba: &MononokeScubaSampleBuilder,
    hook_name: &str,
    cs: &BonsaiChangeset,
    bypass_reason: &str,
) {
    cloned!(mut scuba);
    scuba.add("hook", hook_name.to_string());
    scuba.add("hash", cs.get_changeset_id().to_string());
    scuba.add("bypass_reason", bypass_reason.to_string());
    scuba.log();
}

fn get_bypassed_by_pushvar_reason(
    bypass: Option<&HookBypass>,
    maybe_pushvars: Option<&HashMap<String, Bytes>>,
) -> Option<String> {
    let bypass = bypass?;
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

fn get_bypassed_by_commit_msg_reason(bypass: Option<&HookBypass>, cs_msg: &str) -> Option<String> {
    let bypass = bypass?;

    if let Some(bypass_string) = bypass.commit_message_bypass() {
        if cs_msg.contains(bypass_string) {
            return Some(format!("bypass string: {}", bypass_string));
        }
    }

    None
}

pub enum HooksOutcome<'a> {
    Individual(Vec<BoxFuture<'a, Result<HookOutcome, Error>>>),
    Batched(Vec<BoxFuture<'a, Result<HookOutcome, Error>>>),
}

impl<'a> HooksOutcome<'a> {
    fn is_batched(&self) -> bool {
        match self {
            Self::Individual(_) => false,
            Self::Batched(_) => true,
        }
    }
    fn into_inner(self) -> Vec<BoxFuture<'a, Result<HookOutcome, Error>>> {
        match self {
            Self::Individual(x) => x,
            Self::Batched(x) => x,
        }
    }
}

enum Hook {
    Bookmark(Box<dyn BookmarkHook>, HookConfig),
    Changeset(Box<dyn ChangesetHook>, HookConfig),
    File(Box<dyn FileHook>, HookConfig),
}

pub(crate) enum HookInstance<'a> {
    Bookmark(&'a dyn BookmarkHook),
    Changeset(&'a dyn ChangesetHook),
    File(
        &'a dyn FileHook,
        &'a NonRootMPath,
        Option<&'a BasicFileChange>,
    ),
}

impl<'a> HookInstance<'a> {
    fn run_changeset_hook_on_many_changesets(
        self,
        ctx: &'a CoreContext,
        bookmark: &'a BookmarkKey,
        content_provider: &'a dyn HookStateProvider,
        hook_name: &'a str,
        scuba: MononokeScubaSampleBuilder,
        changesets: Vec<&'a BonsaiChangeset>,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
        log_only: bool,
    ) -> HooksOutcome<'a> {
        match self {
            Self::Bookmark(..) | Self::File(..) => {
                // For bookmarks, we don't need batching as we run one per hook per push
                // For files, the instance is specific to a changeset, so batching on changesets
                // doesn't make much sense
                // This should never be called
                HooksOutcome::Individual(Vec::new())
            }
            Self::Changeset(hook) => hook.run_hook_on_many_changesets(
                ctx,
                bookmark,
                changesets,
                content_provider,
                cross_repo_push_source,
                push_authored_by,
                hook_name,
                scuba,
                log_only,
            ),
        }
    }
    pub(crate) async fn run_hook(
        self,
        ctx: &CoreContext,
        bookmark: &BookmarkKey,
        content_provider: &dyn HookStateProvider,
        hook_name: &str,
        scuba: MononokeScubaSampleBuilder,
        cs: &BonsaiChangeset,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
        log_only: bool,
    ) -> Result<HookOutcome, Error> {
        let cs_id = cs.get_changeset_id();
        match self {
            Self::Bookmark(hook) => hook.run_hook(
                ctx,
                bookmark,
                cs,
                content_provider,
                cross_repo_push_source,
                push_authored_by,
                hook_name,
                scuba,
                log_only,
            ),
            Self::Changeset(hook) => hook.run_hook(
                ctx,
                bookmark,
                cs,
                content_provider,
                cross_repo_push_source,
                push_authored_by,
                hook_name,
                scuba,
                log_only,
            ),
            Self::File(hook, path, change) => hook.run_hook(
                ctx,
                content_provider,
                change,
                path,
                cross_repo_push_source,
                push_authored_by,
                cs_id,
                hook_name,
                scuba,
                log_only,
            ),
        }
        .await
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
    ) -> Vec<impl Future<Output = Result<HookOutcome, Error>> + 'cs> {
        let mut futures = Vec::new();

        match self {
            Self::Bookmark(hook, _) => futures.push(HookInstance::Bookmark(&**hook).run_hook(
                ctx,
                bookmark,
                content_provider,
                hook_name,
                scuba,
                to,
                cross_repo_push_source,
                push_authored_by,
                log_only,
            )),
            Self::Changeset(..) | Self::File(..) =>
                /* Not a bookmark hook */
                {}
        };
        futures
    }

    pub fn get_futures_for_changeset_or_file_hooks<'a: 'cs, 'cs>(
        &'a self,
        ctx: &'a CoreContext,
        bookmark: &'a BookmarkKey,
        content_provider: &'a dyn HookStateProvider,
        hook_name: &'cs str,
        changesets: Vec<&'cs BonsaiChangeset>,
        scuba: MononokeScubaSampleBuilder,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
        log_only: bool,
    ) -> HooksOutcome<'cs> {
        match self {
            Self::Changeset(hook, _) => HookInstance::Changeset(&**hook)
                .run_changeset_hook_on_many_changesets(
                    ctx,
                    bookmark,
                    content_provider,
                    hook_name,
                    scuba,
                    changesets,
                    cross_repo_push_source,
                    push_authored_by,
                    log_only,
                ),
            Self::File(hook, _) => HooksOutcome::Individual(
                changesets
                    .iter()
                    .flat_map(|cs| {
                        cloned!(mut scuba);
                        cs.simplified_file_changes()
                            .map(move |(path, change)| {
                                HookInstance::File(&**hook, path, change)
                                    .run_hook(
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
                                    .boxed()
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect(),
            ),
            Self::Bookmark(..) =>
            /* Not a changeset or file hook */
            {
                HooksOutcome::Individual(Vec::new())
            }
        }
    }
}

#[cfg(test)]
mod test {
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn test_commit_message_bypass() {
        let bypass = HookBypass::new_with_commit_msg("@mybypass".into());

        let r = get_bypassed_by_commit_msg_reason(Some(&bypass), "@notbypass");
        assert!(r.is_none());

        let r = get_bypassed_by_commit_msg_reason(Some(&bypass), "foo @mybypass bar");
        assert!(r.is_some());
    }

    #[mononoke::test]
    fn test_pushvar_bypass() {
        let bypass = HookBypass::new_with_pushvar("myvar".into(), "myvalue".into());

        let mut m = HashMap::new();
        let r = get_bypassed_by_pushvar_reason(Some(&bypass), Some(&m));
        assert!(r.is_none()); // No var

        m.insert("somevar".into(), "somevalue".as_bytes().into());
        let r = get_bypassed_by_pushvar_reason(Some(&bypass), Some(&m));
        assert!(r.is_none()); // wrong var

        m.insert("myvar".into(), "somevalue".as_bytes().into());
        let r = get_bypassed_by_pushvar_reason(Some(&bypass), Some(&m));
        assert!(r.is_none()); // wrong value

        m.insert("myvar".into(), "myvalue foo".as_bytes().into());
        let r = get_bypassed_by_pushvar_reason(Some(&bypass), Some(&m));
        assert!(r.is_none()); // wrong value

        m.insert("myvar".into(), "myvalue".as_bytes().into());
        let r = get_bypassed_by_pushvar_reason(Some(&bypass), Some(&m));
        assert!(r.is_some());
    }
}
