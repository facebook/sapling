/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::str;
use std::sync::Arc;
use std::sync::LazyLock;

use anyhow::Error;
use anyhow::Result;
use bookmarks_types::AnnotatedTags;
use bookmarks_types::BookmarkKey;
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
#[cfg(fbcode_build)]
use employee_service::MononokeEmployeeService;
#[cfg(fbcode_build)]
use employee_service::prod::ProdEmployeeService;
use fbinit::FacebookInit;
use futures::Future;
use futures::FutureExt;
use futures::future::BoxFuture;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::stream::futures_unordered::FuturesUnordered;
use futures::try_join;
use metaconfig_types::BookmarkOrRegex;
use metaconfig_types::HookBypassCondition;
use metaconfig_types::HookConfig;
use metaconfig_types::HookManagerParams;
use mononoke_types::BasicFileChange;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::NonRootMPath;
use permission_checker::AclProvider;
use permission_checker::ArcMembershipChecker;
use permission_checker::MononokeIdentity;
use permission_checker::MononokeIdentitySet;
use permission_checker::NeverMember;
use regex::Regex;
use repo_permission_checker::ArcRepoPermissionChecker;
use repo_permission_checker::NeverAllowRepoPermissionChecker;
use scuba::builder::ServerData;
use scuba_ext::MononokeScubaSampleBuilder;
use tracing::debug;

use crate::BookmarkHook;
use crate::BypassDecision;
use crate::BypassIdentitySource;
use crate::ChangesetHook;
use crate::CheckedBypassIdentities;
use crate::CrossRepoPushSource;
use crate::FileHook;
use crate::HookExecution;
use crate::HookExecutionPurpose;
use crate::HookOutcome;
use crate::HookRepo;
use crate::LogOnlyRejections;
use crate::PushAuthoredBy;
use crate::errors::HookManagerError;

/// Manages hooks and allows them to be installed and uninstalled given a name
/// Knows how to run hooks

#[facet::facet]
pub struct HookManager {
    repo_name: String,
    hooks: HashMap<String, Hook>,
    bookmark_hooks: HashMap<BookmarkKey, Vec<String>>,
    regex_hooks: Vec<(Arc<Regex>, Vec<String>)>,
    inverse_regex_hooks: Vec<(Arc<Regex>, Vec<String>)>,
    repo: HookRepo,
    reviewers_membership: ArcMembershipChecker,
    admin_membership: ArcMembershipChecker,
    repo_permission_checker: ArcRepoPermissionChecker,
    scuba: MononokeScubaSampleBuilder,
    all_hooks_bypassed: bool,
    scuba_bypassed_commits: MononokeScubaSampleBuilder,
    /// Resolves a commit author's email to their canonical unixname. `None` on
    /// the test/disabled-ACL path.
    #[cfg(fbcode_build)]
    employee_service: Option<Arc<dyn MononokeEmployeeService + Send + Sync>>,
}

/// Whether the pusher is a member of a bypass condition's permission group,
/// with the identities the check ran against, so the decision can be logged and
/// explained in terms of *whose* membership was tested.
#[derive(Clone)]
enum GroupMembership {
    /// The pusher is a member.
    Member(CheckedBypassIdentities),
    /// The pusher is not a member.
    NotMember(CheckedBypassIdentities),
}

impl HookManager {
    pub async fn new(
        fb: FacebookInit,
        acl_provider: &dyn AclProvider,
        repo: HookRepo,
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

        // No production services when ACL checks are disabled (test path).
        #[cfg(fbcode_build)]
        let employee_service: Option<Arc<dyn MononokeEmployeeService + Send + Sync>> =
            if hook_manager_params.disable_acl_checker {
                None
            } else {
                Some(Arc::new(ProdEmployeeService::new(fb)?))
            };

        Ok(HookManager {
            repo_name,
            hooks,
            bookmark_hooks: HashMap::new(),
            regex_hooks: Vec::new(),
            inverse_regex_hooks: Vec::new(),
            repo,
            reviewers_membership: reviewers_membership.into(),
            admin_membership: admin_membership.into(),
            scuba,
            all_hooks_bypassed: hook_manager_params.all_hooks_bypassed,
            scuba_bypassed_commits,
            repo_permission_checker,
            #[cfg(fbcode_build)]
            employee_service,
        })
    }

    // Create a very simple HookManager, for use inside of the TestRepoFactory.
    pub fn new_test(repo_name: String, repo: HookRepo) -> Self {
        Self {
            repo_name,
            hooks: HashMap::new(),
            bookmark_hooks: HashMap::new(),
            regex_hooks: Vec::new(),
            inverse_regex_hooks: Vec::new(),
            repo,
            reviewers_membership: NeverMember::new().into(),
            admin_membership: NeverMember::new().into(),
            scuba: MononokeScubaSampleBuilder::with_discard(),
            all_hooks_bypassed: false,
            scuba_bypassed_commits: MononokeScubaSampleBuilder::with_discard(),
            repo_permission_checker: Arc::new(NeverAllowRepoPermissionChecker {}),
            #[cfg(fbcode_build)]
            employee_service: None,
        }
    }

    /// Test-only: inject a mock EmployeeService for the bypass canonical-unixname
    /// resolution path.
    #[cfg(all(test, fbcode_build))]
    pub(crate) fn set_employee_service_for_test(
        &mut self,
        service: Arc<dyn MononokeEmployeeService + Send + Sync>,
    ) {
        self.employee_service = Some(service);
    }

    /// Test-only: resolve the eager bypass decision for a registered hook.
    /// `hooks` is private to this module, and the decision is otherwise only
    /// observable through the Scuba columns it produces.
    #[cfg(test)]
    pub(crate) async fn compute_bypass_decision_for_test(
        &self,
        hook_name: &str,
        ctx: &CoreContext,
        maybe_pushvars: Option<&HashMap<String, Bytes>>,
        cs_msg: Option<&str>,
        changeset_author: Option<&str>,
    ) -> Result<BypassDecision> {
        let hook = self
            .hooks
            .get(hook_name)
            .ok_or_else(|| HookManagerError::NoSuchHook(hook_name.to_string()))?;
        self.compute_bypass_decision(hook, ctx, maybe_pushvars, cs_msg, changeset_author)
            .await
    }

    pub fn register_bookmark_hook(
        &mut self,
        hook_name: &str,
        hook: Box<dyn BookmarkHook>,
        config: HookConfig,
        bypass_checkers: BypassCheckers,
    ) {
        self.hooks.insert(
            hook_name.to_string(),
            Hook::from_bookmark(hook, config, bypass_checkers),
        );
    }

    pub fn register_changeset_hook(
        &mut self,
        hook_name: &str,
        hook: Box<dyn ChangesetHook>,
        config: HookConfig,
        bypass_checkers: BypassCheckers,
    ) {
        self.hooks.insert(
            hook_name.to_string(),
            Hook::from_changeset(hook, config, bypass_checkers),
        );
    }

    pub fn register_file_hook(
        &mut self,
        hook_name: &str,
        hook: Box<dyn FileHook>,
        config: HookConfig,
        bypass_checkers: BypassCheckers,
    ) {
        self.hooks.insert(
            hook_name.to_string(),
            Hook::from_file(hook, config, bypass_checkers),
        );
    }

    /// Compute the bypass decision for a single surviving `(hook, changeset)`
    /// pair, eagerly (before the hook runs).
    ///
    /// The first condition that matches authorizes the bypass. When none does,
    /// the decision describes the first condition whose markers are present (a
    /// condition with no markers always counts), so the rejection can say why
    /// the bypass was ignored. A condition's requirements are checked in order:
    /// permission group, identities, then actor. The agent signal comes from the
    /// pusher's own metadata: the identity a membership check runs against is
    /// often the commit author's synthetic `USER:<unixname>`, which never carries
    /// the credential attributes `likely_an_agent` looks for.
    async fn compute_bypass_decision(
        &self,
        hook: &Hook,
        ctx: &CoreContext,
        maybe_pushvars: Option<&HashMap<String, Bytes>>,
        cs_msg: Option<&str>,
        changeset_author: Option<&str>,
    ) -> Result<BypassDecision> {
        let is_agent = ctx.metadata().likely_an_agent();
        let mut memberships: HashMap<&str, GroupMembership> = HashMap::new();
        let mut near_miss = None;
        for (condition_index, condition) in hook.get_config().bypasses.iter().enumerate() {
            // Cheap synchronous trigger check first: no trigger means no ACL call.
            let Some(reason) = condition_trigger_reason(condition, maybe_pushvars, cs_msg) else {
                continue;
            };
            let permission_group = condition.permission_group.clone();
            let check = match condition.permission_group.as_deref() {
                None => None,
                Some(group) => {
                    let membership = match memberships.get(group) {
                        Some(membership) => membership.clone(),
                        None => {
                            let membership = self
                                .check_bypass_group_membership(hook, group, ctx, changeset_author)
                                .await?;
                            memberships.insert(group, membership.clone());
                            membership
                        }
                    };
                    match membership {
                        GroupMembership::Member(check) => Some(check),
                        GroupMembership::NotMember(check) => {
                            near_miss.get_or_insert(BypassDecision::UnauthorizedUser {
                                condition: condition_index,
                                reason,
                                group: group.to_string(),
                                check,
                            });
                            continue;
                        }
                    }
                }
            };
            if !has_bypass_identity(condition, ctx) {
                near_miss.get_or_insert(BypassDecision::UnauthorizedIdentity {
                    condition: condition_index,
                    reason,
                    permission_group,
                    check,
                });
                continue;
            }
            if !condition.actor.allows(is_agent) {
                near_miss.get_or_insert(if is_agent {
                    BypassDecision::UnauthorizedAgent {
                        condition: condition_index,
                        reason,
                        permission_group,
                        check,
                    }
                } else {
                    BypassDecision::UnauthorizedHuman {
                        condition: condition_index,
                        reason,
                        permission_group,
                        check,
                    }
                });
                continue;
            }
            return Ok(BypassDecision::Authorized {
                condition: condition_index,
                reason,
                permission_group,
                check,
            });
        }
        Ok(near_miss.unwrap_or_default())
    }

    /// Check whether the pusher is a member of `group`, one of the hook's
    /// bypass permission groups.
    ///
    /// Membership is checked against the commit author's identity when
    /// `changeset_author` is provided (e.g., "Alice <alice@fb.com>"), rather
    /// than the pusher's TLS cert identities. On the client-identities path the
    /// pusher's request identities are checked first, and a member wins
    /// outright; otherwise the commit author is checked, including the
    /// canonical unixname EmployeeService re-check. An empty identity set fails
    /// closed, and so does a group the hook has no membership checker for.
    async fn check_bypass_group_membership(
        &self,
        hook: &Hook,
        group: &str,
        ctx: &CoreContext,
        changeset_author: Option<&str>,
    ) -> Result<GroupMembership> {
        let Some(checker) = hook.get_bypass_permission_checker(group) else {
            tracing::error!(
                group,
                "No membership checker for hook bypass permission group; refusing the bypass"
            );
            return Ok(GroupMembership::NotMember(CheckedBypassIdentities {
                source: BypassIdentitySource::NoChecker,
                identities: Vec::new(),
            }));
        };

        let use_client_identities = justknobs::eval(
            "scm/mononoke:check_hook_bypass_permission_group_with_client_identities",
            None,
            Some(self.repo_name.as_str()),
        );
        if use_client_identities {
            let membership = check_membership(
                checker,
                ctx.metadata().identities(),
                BypassIdentitySource::ClientIdentities,
            )
            .await;
            if matches!(membership, GroupMembership::Member(_)) {
                return Ok(membership);
            }
        }

        Ok(self
            .check_changeset_author_membership(checker, ctx, changeset_author)
            .await)
    }

    /// Check the commit author's identity against `checker`, falling back to
    /// the pusher's client identities when the author is missing or
    /// unparsable. When the author's `USER:<local-part>` identity is not a
    /// member, resolve their canonical unixname via the EmployeeService
    /// (JK-gated) and check that instead -- this handles authors whose email
    /// local-part differs from their unixname. The re-check can only ever
    /// upgrade `NotMember` -> `Member`; on any miss or error the original
    /// decision stands.
    async fn check_changeset_author_membership(
        &self,
        checker: &ArcMembershipChecker,
        ctx: &CoreContext,
        changeset_author: Option<&str>,
    ) -> GroupMembership {
        let Some(author_identity) = changeset_author.and_then(extract_identity_from_author) else {
            // The identities checked here belong to the pusher, not the
            // author -- for a bot-authored commit pushed by a service they are
            // unrelated. `CommitAuthorFallbackToClient` is what makes that
            // visible instead of looking like a plain author check.
            return check_membership(
                checker,
                ctx.metadata().identities(),
                BypassIdentitySource::CommitAuthorFallbackToClient,
            )
            .await;
        };

        let is_user = author_identity.id_type() == "USER";
        let membership = check_membership(
            checker,
            &std::iter::once(author_identity).collect(),
            BypassIdentitySource::CommitAuthor,
        )
        .await;

        if matches!(membership, GroupMembership::NotMember(_))
            && is_user
            && justknobs::eval(
                "scm/mononoke:resolve_unixname_from_employee_service_for_hook_bypass",
                None,
                Some(self.repo_name.as_str()),
            )
        {
            if let Some(unixname) = self.resolve_author_unixname(changeset_author).await {
                let identity = MononokeIdentity::from_legacy_type_data("USER", &unixname);
                return check_membership(
                    checker,
                    &std::iter::once(identity).collect(),
                    BypassIdentitySource::CommitAuthorResolvedUnixname,
                )
                .await;
            }
        }
        membership
    }

    /// Resolve the commit author's canonical unixname from their email via the
    /// EmployeeService. `None` on any miss/error (logged) so callers fail closed.
    #[cfg(fbcode_build)]
    async fn resolve_author_unixname(&self, author: Option<&str>) -> Option<String> {
        let email = author_email(author?)?;
        let service = self.employee_service.as_ref()?;
        match service.email_to_unixname(&email).await {
            Ok(unixname) => unixname,
            Err(e) => {
                tracing::warn!(
                    "hook bypass: unixname resolution failed for author '{email}': {e:?}; \
                     keeping unauthorized decision"
                );
                None
            }
        }
    }

    /// OSS builds have no EmployeeService, so the re-check is a no-op.
    #[cfg(not(fbcode_build))]
    async fn resolve_author_unixname(&self, _author: Option<&str>) -> Option<String> {
        None
    }

    pub fn set_hooks_for_bookmark(&mut self, bookmark: BookmarkOrRegex, hooks: Vec<String>) {
        match bookmark {
            BookmarkOrRegex::Bookmark(bookmark) => {
                self.bookmark_hooks.insert(bookmark, hooks);
            }
            BookmarkOrRegex::Regex(regex) => {
                self.regex_hooks.push((regex.into_inner(), hooks));
            }
            BookmarkOrRegex::InverseRegex(regex) => {
                self.inverse_regex_hooks.push((regex.into_inner(), hooks));
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

        let bookmark_str = bookmark.as_str();

        // Check regular regex hooks
        if self
            .regex_hooks
            .iter()
            .any(|(regex, _)| regex.is_match(bookmark_str))
        {
            return true;
        }

        // Check inverse regex hooks (match if regex does NOT match)
        self.inverse_regex_hooks
            .iter()
            .any(|(regex, _)| !regex.is_match(bookmark_str))
    }

    pub fn repo_name(&self) -> &String {
        &self.repo_name
    }

    fn hooks_for_bookmark<'a>(
        &'a self,
        bookmark: &BookmarkKey,
    ) -> impl Iterator<Item = &'a str> + Clone + use<'a> {
        let mut hooks: Vec<&'a str> = match self.bookmark_hooks.get(bookmark) {
            Some(hooks) => hooks.iter().map(|a| a.as_str()).collect(),
            None => Vec::new(),
        };

        let bookmark_str = bookmark.to_string();

        // Add hooks from regular regex patterns
        for (regex, r_hooks) in &self.regex_hooks {
            if regex.is_match(&bookmark_str) {
                hooks.extend(r_hooks.iter().map(|a| a.as_str()));
            }
        }

        // Add hooks from inverse regex patterns (match if regex does NOT match)
        for (regex, ir_hooks) in &self.inverse_regex_hooks {
            if !regex.is_match(&bookmark_str) {
                hooks.extend(ir_hooks.iter().map(|a| a.as_str()));
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
        annotated_tags: Option<&AnnotatedTags>,
    ) -> Result<Vec<HookOutcome>, Error> {
        debug!("Running bookmark hooks for bookmark {:?}", bookmark);

        let hooks = self.hooks_for_bookmark(bookmark);

        let futs = FuturesUnordered::new();

        let mut scuba = self.scuba.clone();
        let username = ctx.metadata().unix_name();
        let user_option = ctx.metadata().client_hostname().or(username);

        if let Some(user) = user_option {
            scuba.add("user", user);
        }
        if let Some(cri) = ctx.metadata().client_request_info() {
            scuba.add_client_request_info(cri);
        }
        scuba.add(
            "originating_purpose",
            HookExecutionPurpose::LandAttempt.as_str(),
        );

        for hook_name in hooks {
            let hook = self
                .hooks
                .get(hook_name)
                .ok_or_else(|| HookManagerError::NoSuchHook(hook_name.to_string()))?;

            let mut scuba = scuba.clone();
            scuba.add("hook", hook_name.to_string());
            scuba.add("to", to.get_changeset_id().to_string());

            // A condition with no permission group is honored without running the
            // hook, skipped pre-run. Bookmark hooks have no commit message, so
            // conditions that require one never match them.
            if let Some((condition, reason)) = unconditional_bypass(hook, ctx, maybe_pushvars, None)
            {
                scuba.add("type", "bookmark");
                log_skipped_hook(&scuba, to, condition, &reason);
                continue;
            }

            // For a surviving hook, decide the bypass eagerly so the hook's row is
            // logged once with its final outcome.
            let bypass = self
                .compute_bypass_decision(hook, ctx, maybe_pushvars, None, Some(to.author()))
                .await?;

            for future in hook.get_futures_for_bookmark_hooks(
                ctx,
                &self.repo,
                bookmark,
                hook_name,
                to,
                scuba,
                cross_repo_push_source,
                push_authored_by,
                annotated_tags,
                maybe_pushvars,
                hook.get_config().log_only,
                bypass,
            ) {
                futs.push(future.boxed());
            }
        }
        let outcomes: Vec<HookOutcome> = futs.try_collect().await?;
        Ok(outcomes)
    }

    pub async fn run_changesets_hooks_for_bookmark(
        &self,
        ctx: &CoreContext,
        changesets: &[BonsaiChangeset],
        bookmark: &BookmarkKey,
        maybe_pushvars: Option<&HashMap<String, Bytes>>,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
        purpose: HookExecutionPurpose,
        // When the hooks are being run as a different set of identities than the
        // caller's (e.g. `commit_run_hooks` with `run_as`), this holds the
        // caller's real identities for audit logging. `None` for real pushes.
        run_as_original_identities: Option<&MononokeIdentitySet>,
        log_only_rejections: LogOnlyRejections,
    ) -> Result<Vec<HookOutcome>, Error> {
        debug!("Running hooks for bookmark {:?}", bookmark);

        let hooks = self.hooks_for_bookmark(bookmark);

        let mut scuba = self.scuba.clone();
        let username = ctx.metadata().unix_name();
        let user_option = ctx.metadata().client_hostname().or(username);

        if let Some(user) = user_option {
            scuba.add("user", user);
        }
        if let Some(cri) = ctx.metadata().client_request_info() {
            scuba.add_client_request_info(cri);
        }
        scuba.add("originating_purpose", purpose.as_str());

        if let Some(user) = user_option {
            scuba.add("user", user);
        }

        // Record when hooks are run as a different set of identities than the
        // real caller (e.g. `commit_run_hooks` with `run_as`) so the scm_hooks
        // scuba table captures both that run_as was used and who the real caller
        // was. `scuba` propagates into each hook's per-execution log below.
        if let Some(original_identities) = run_as_original_identities {
            scuba.add("run_as", true);
            scuba.add(
                "original_identities_when_run_as",
                original_identities
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>(),
            );
        }

        let resolved_hooks = hooks
            .map(|hook_name| {
                let hook = self
                    .hooks
                    .get(hook_name)
                    .ok_or_else(|| HookManagerError::NoSuchHook(hook_name.to_string()))?;
                Ok((hook_name, hook))
            })
            .collect::<Result<Vec<_>>>()?;

        // Drop each (hook, changeset) that a condition with no permission group
        // bypasses, logging it as skipped.
        let surviving: Vec<(&str, &Hook, Vec<&BonsaiChangeset>)> = resolved_hooks
            .into_iter()
            .map(|(hook_name, hook)| {
                let css = changesets
                    .iter()
                    .filter(|cs| {
                        match unconditional_bypass(hook, ctx, maybe_pushvars, Some(cs.message())) {
                            Some((condition, reason)) => {
                                let mut scuba = scuba.clone();
                                scuba.add("hook", hook_name.to_string());
                                scuba.add("type", hook.scuba_type());
                                log_skipped_hook(&scuba, cs, condition, &reason);
                                false
                            }
                            None => true,
                        }
                    })
                    .collect();
                (hook_name, hook, css)
            })
            .collect();

        // Resolve each survivor's bypass decision eagerly, with bounded
        // concurrency: most pairs are cheap (no bypass token means no ACL call), but
        // a fired token costs a membership RPC we don't want to serialize on the
        // push critical path.
        const BYPASS_DECISION_CONCURRENCY: usize = 10;
        let decision_futs: Vec<BoxFuture<Result<(usize, ChangesetId, BypassDecision)>>> = surviving
            .iter()
            .enumerate()
            .flat_map(|(idx, (_, hook, css))| {
                css.iter().map(move |cs| {
                    let cs_id = cs.get_changeset_id();
                    self.compute_bypass_decision(
                        hook,
                        ctx,
                        maybe_pushvars,
                        Some(cs.message()),
                        Some(cs.author()),
                    )
                    .map(move |res| res.map(|bypass| (idx, cs_id, bypass)))
                    .boxed()
                })
            })
            .collect();
        let decisions: Vec<(usize, ChangesetId, BypassDecision)> =
            futures::stream::iter(decision_futs)
                .buffer_unordered(BYPASS_DECISION_CONCURRENCY)
                .try_collect()
                .await?;

        let mut bypass_maps: Vec<HashMap<ChangesetId, BypassDecision>> =
            surviving.iter().map(|_| HashMap::new()).collect();
        for (idx, cs_id, bypass) in decisions {
            bypass_maps[idx].insert(cs_id, bypass);
        }
        let hooks_with_changesets: Vec<_> = surviving
            .into_iter()
            .zip(bypass_maps)
            .map(|((hook_name, hook, css), bypass_by_cs)| {
                (hook_name, hook, css, Arc::new(bypass_by_cs))
            })
            .collect();

        let (batched, individual) = hooks_with_changesets
            .into_iter()
            .map(|(hook_name, hook, changesets, bypass_by_cs)| {
                cloned!(mut scuba);
                scuba.add("hook", hook_name.to_string());
                hook.get_futures_for_changeset_or_file_hooks(
                    ctx,
                    &self.repo,
                    bookmark,
                    hook_name,
                    changesets,
                    scuba,
                    cross_repo_push_source,
                    push_authored_by,
                    purpose,
                    maybe_pushvars,
                    log_only_rejections.suppresses(hook.get_config().log_only),
                    bypass_by_cs,
                )
            })
            .partition::<Vec<_>, _>(HooksOutcome::is_batched);

        let individual_concurrency = justknobs::get_as::<usize>(
            "scm/mononoke:bookmark_movement_changeset_hooks_individual_concurency",
            Some(&self.repo_name),
        );
        let batched_concurrency = justknobs::get_as::<usize>(
            "scm/mononoke:bookmark_movement_changeset_hooks_batched_concurency",
            Some(&self.repo_name),
        );

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
        let outcomes: Vec<HookOutcome> = individual_res.into_iter().chain(batched_res).collect();
        Ok(outcomes)
    }
}

/// Append a note to a rejection telling the pusher their bypass was ignored
/// because they are not in the permission group. The hook's own reason is kept.
///
/// The identities actually tested are deliberately not named here -- they go to
/// the `scm_hooks` Scuba table (`bypass_identities_checked`,
/// `bypass_identity_source`) for debugging instead of into the pusher's output.
pub(crate) fn annotate_unauthorized_rejection(
    outcome: HookOutcome,
    group_name: &str,
) -> HookOutcome {
    annotate_rejection(
        outcome,
        &format!(
            "Note: your hook bypass was ignored because you are not a member of \
             group '{group_name}'. Request access to the group, or fix the issue above."
        ),
        "Unauthorized bypass rejected",
    )
}

/// Append a note to a rejection telling the pusher their bypass was ignored
/// because it is restricted to humans and the pusher is an agent. The hook's
/// own reason is kept.
pub(crate) fn annotate_agent_bypass_rejection(
    outcome: HookOutcome,
    group_name: Option<&str>,
) -> HookOutcome {
    let restriction = match group_name {
        Some(group_name) => format!(
            "bypassing this hook is restricted to members of group '{group_name}', and \
             that call cannot be made by an agent"
        ),
        None => "this bypass cannot be used by an agent".to_string(),
    };
    annotate_rejection(
        outcome,
        &format!(
            "Note: your hook bypass was ignored because {restriction}. A human needs to \
             review these changes and, if the bypass is warranted, push them themselves."
        ),
        "Agent bypass rejected",
    )
}

/// Append a note to a rejection telling the pusher their bypass was ignored
/// because it is restricted to agents. The hook's own reason is kept.
pub(crate) fn annotate_human_bypass_rejection(outcome: HookOutcome) -> HookOutcome {
    annotate_rejection(
        outcome,
        "Note: your hook bypass was ignored because it can only be used by an agent.",
        "Human bypass rejected",
    )
}

/// Append a note to a rejection telling the pusher their bypass was ignored
/// because none of their identities may use it. The hook's own reason is kept,
/// and the allowed identities are not named.
pub(crate) fn annotate_identity_bypass_rejection(outcome: HookOutcome) -> HookOutcome {
    annotate_rejection(
        outcome,
        "Note: your hook bypass was ignored because you are not one of the identities \
         allowed to use it.",
        "Bypass identity not allowed",
    )
}

/// Append `note` to a rejection's user-facing description and prepend
/// `extra_log` to its diagnostic logs. A no-op on an accepted outcome.
fn annotate_rejection(mut outcome: HookOutcome, note: &str, extra_log: &str) -> HookOutcome {
    if let Some(info) = outcome.get_execution().rejection_info() {
        let mut info = info.clone();
        info.long_description = format!("{}\n\n{note}", info.long_description);
        let extra_logs = [extra_log.to_string()]
            .into_iter()
            .chain(outcome.get_execution().extra_logs.clone())
            .collect();
        outcome.set_execution(HookExecution::rejected_with_logs(info, extra_logs));
    }
    outcome
}

/// Matches a changeset author of the form `Name <local-part@host>`, capturing
/// the local-part and host.
static AUTHOR_RE: LazyLock<Regex> = LazyLock::new(|| {
    regex::RegexBuilder::new(".*<(.+)@(.+)>")
        .case_insensitive(true)
        .build()
        .expect("valid regex")
});

/// Extract the `local-part@host` email from a changeset author "Name <user@host>".
/// Used to resolve the author's canonical unixname via the EmployeeService.
#[cfg(fbcode_build)]
fn author_email(author: &str) -> Option<String> {
    let caps = AUTHOR_RE.captures(author)?;
    Some(format!(
        "{}@{}",
        caps.get(1)?.as_str(),
        caps.get(2)?.as_str()
    ))
}

/// Build a MononokeIdentity from a changeset author like "Name <user@host>".
/// "noreply+<FBID>@fb.com" bot authors yield an FBID identity; everything else
/// yields USER:<local-part>.
fn extract_identity_from_author(author: &str) -> Option<MononokeIdentity> {
    let caps = AUTHOR_RE.captures(author)?;
    let local_part = caps.get(1)?.as_str();
    let host = caps.get(2)?.as_str();

    let is_internal = matches!(host.to_ascii_lowercase().as_str(), "fb.com" | "meta.com");
    // Codemod bots are Meiosis service users with no unixname; their FBID only
    // reaches us via the "noreply+<FBID>" author local-part, so recover it here.
    let fbid = local_part
        .strip_prefix("noreply+")
        .filter(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()));

    Some(match fbid {
        Some(fbid) if is_internal => MononokeIdentity::from_legacy_type_data("FBID", fbid),
        _ => MononokeIdentity::from_legacy_type_data("USER", local_part),
    })
}

/// Log a hook that was skipped before it ran because of an unconditional
/// (permission-group-less) pushvar/commit-message bypass. The caller adds the
/// `hook` and `type` columns; this records the changeset, author, the bypass
/// reason, and `outcome=skipped` so the skip is a self-describing per-hook row.
fn log_skipped_hook(
    scuba: &MononokeScubaSampleBuilder,
    cs: &BonsaiChangeset,
    condition: usize,
    bypass_reason: &str,
) {
    cloned!(mut scuba);
    scuba.add("changeset_id", cs.get_changeset_id().to_string());
    scuba.add("hash", cs.get_changeset_id().to_string());
    scuba.add("author", cs.author().to_string());
    scuba.add("bypass_condition", condition as i64);
    scuba.add("bypass_reason", bypass_reason.to_string());
    scuba.add("outcome", "skipped");
    scuba.log();
}

/// The first bypass condition that needs no membership check (it has no
/// permission group) and matches, with its index and the reason, so the hook
/// can be skipped before it runs. Conditions with a group are decided eagerly
/// by `compute_bypass_decision` and carried into the hook-run path. Skipping
/// pre-run is also what keeps a group-less bypass able to rescue a hook that
/// would otherwise error (its `Err` would abort the push before its outcome is
/// logged).
fn unconditional_bypass(
    hook: &Hook,
    ctx: &CoreContext,
    maybe_pushvars: Option<&HashMap<String, Bytes>>,
    cs_msg: Option<&str>,
) -> Option<(usize, String)> {
    let is_agent = ctx.metadata().likely_an_agent();
    hook.get_config()
        .bypasses
        .iter()
        .enumerate()
        .filter(|(_, condition)| {
            condition.permission_group.is_none()
                && condition.actor.allows(is_agent)
                && has_bypass_identity(condition, ctx)
        })
        .find_map(|(index, condition)| {
            condition_trigger_reason(condition, maybe_pushvars, cs_msg)
                .map(|reason| (index, reason))
        })
}

/// Why a bypass condition's markers are present in the push: its pushvar and
/// its commit message marker, or, for a condition with neither, its permission
/// group or identities alone. `None` when a marker it requires is missing.
fn condition_trigger_reason(
    condition: &HookBypassCondition,
    maybe_pushvars: Option<&HashMap<String, Bytes>>,
    cs_msg: Option<&str>,
) -> Option<String> {
    let mut reasons = Vec::new();
    if let Some((name, value)) = &condition.pushvar {
        let pushvar_value = maybe_pushvars?.get(name)?;
        if pushvar_value.as_ref() != value.as_bytes() {
            return None;
        }
        reasons.push(format!("bypass pushvar: {name}={value}"));
    }
    if let Some(marker) = &condition.commit_message {
        if !cs_msg?.contains(marker.as_str()) {
            return None;
        }
        reasons.push(format!("bypass string: {marker}"));
    }
    if reasons.is_empty() {
        reasons.push(match &condition.permission_group {
            Some(group) => format!("implicit: member of {group}"),
            None => "implicit: allowed identity".to_string(),
        });
    }
    Some(reasons.join(", "))
}

/// Whether the pusher's own identities include one the condition allows, or the
/// condition does not restrict identities.
fn has_bypass_identity(condition: &HookBypassCondition, ctx: &CoreContext) -> bool {
    condition.identities.is_empty()
        || condition
            .identities
            .iter()
            .any(|identity| ctx.metadata().identities().contains(identity))
}

/// Check `identities` against a bypass permission group's `checker`.
async fn check_membership(
    checker: &ArcMembershipChecker,
    identities: &MononokeIdentitySet,
    source: BypassIdentitySource,
) -> GroupMembership {
    let check = CheckedBypassIdentities::new(source, identities);
    if checker.is_member(identities).await {
        GroupMembership::Member(check)
    } else {
        GroupMembership::NotMember(check)
    }
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

/// Membership checkers for a hook's bypass permission groups, by group name.
pub type BypassCheckers = HashMap<String, ArcMembershipChecker>;

enum Hook {
    Bookmark(Box<dyn BookmarkHook>, HookConfig, BypassCheckers),
    Changeset(Box<dyn ChangesetHook>, HookConfig, BypassCheckers),
    File(Box<dyn FileHook>, HookConfig, BypassCheckers),
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
        repo: &'a HookRepo,
        bookmark: &'a BookmarkKey,
        hook_name: &'a str,
        scuba: MononokeScubaSampleBuilder,
        changesets: Vec<&'a BonsaiChangeset>,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
        purpose: HookExecutionPurpose,
        maybe_pushvars: Option<&'a HashMap<String, Bytes>>,
        log_only: bool,
        bypass_by_cs: Arc<HashMap<ChangesetId, BypassDecision>>,
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
                repo,
                bookmark,
                changesets,
                cross_repo_push_source,
                push_authored_by,
                purpose,
                maybe_pushvars,
                hook_name,
                scuba,
                log_only,
                bypass_by_cs,
            ),
        }
    }
    pub(crate) async fn run_hook<'cs>(
        self,
        ctx: &CoreContext,
        repo: &HookRepo,
        bookmark: &BookmarkKey,
        hook_name: &str,
        scuba: MononokeScubaSampleBuilder,
        cs: &'cs BonsaiChangeset,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
        purpose: HookExecutionPurpose,
        annotated_tags: Option<&AnnotatedTags>,
        maybe_pushvars: Option<&'cs HashMap<String, Bytes>>,
        log_only: bool,
        bypass: BypassDecision,
    ) -> Result<HookOutcome, Error> {
        let cs_id = cs.get_changeset_id();
        match self {
            Self::Bookmark(hook) => hook.run_hook(
                ctx,
                repo,
                bookmark,
                cs,
                cross_repo_push_source,
                push_authored_by,
                annotated_tags,
                hook_name,
                scuba,
                log_only,
                bypass,
            ),
            Self::Changeset(hook) => hook.run_hook(
                ctx,
                repo,
                bookmark,
                cs,
                cross_repo_push_source,
                push_authored_by,
                purpose,
                maybe_pushvars,
                hook_name,
                scuba,
                log_only,
                bypass,
            ),
            Self::File(hook, path, change) => hook.run_hook(
                ctx,
                repo,
                change,
                path,
                cross_repo_push_source,
                push_authored_by,
                cs_id,
                hook_name,
                scuba,
                log_only,
                bypass,
            ),
        }
        .await
    }
}

impl Hook {
    pub fn from_bookmark(
        hook: Box<dyn BookmarkHook>,
        config: HookConfig,
        bypass_checkers: BypassCheckers,
    ) -> Self {
        Self::Bookmark(hook, config, bypass_checkers)
    }

    pub fn from_changeset(
        hook: Box<dyn ChangesetHook>,
        config: HookConfig,
        bypass_checkers: BypassCheckers,
    ) -> Self {
        Self::Changeset(hook, config, bypass_checkers)
    }

    pub fn from_file(
        hook: Box<dyn FileHook>,
        config: HookConfig,
        bypass_checkers: BypassCheckers,
    ) -> Self {
        Self::File(hook, config, bypass_checkers)
    }

    pub fn get_config(&self) -> &HookConfig {
        match self {
            Self::Bookmark(_, config, _) => config,
            Self::Changeset(_, config, _) => config,
            Self::File(_, config, _) => config,
        }
    }

    /// The membership checker for one of the hook's bypass permission groups.
    pub fn get_bypass_permission_checker(&self, group: &str) -> Option<&ArcMembershipChecker> {
        match self {
            Self::Bookmark(_, _, checkers)
            | Self::Changeset(_, _, checkers)
            | Self::File(_, _, checkers) => checkers.get(group),
        }
    }

    /// The `type` Scuba column value for this hook, matching what the per-hook
    /// execution rows log ("bookmark"/"changeset"/"file").
    pub(crate) fn scuba_type(&self) -> &'static str {
        match self {
            Self::Bookmark(..) => "bookmark",
            Self::Changeset(..) => "changeset",
            Self::File(..) => "file",
        }
    }

    pub fn get_futures_for_bookmark_hooks<'a: 'cs, 'cs>(
        &'a self,
        ctx: &'a CoreContext,
        repo: &'a HookRepo,
        bookmark: &'a BookmarkKey,
        hook_name: &'cs str,
        to: &'cs BonsaiChangeset,
        scuba: MononokeScubaSampleBuilder,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
        annotated_tags: Option<&'cs AnnotatedTags>,
        maybe_pushvars: Option<&'cs HashMap<String, Bytes>>,
        log_only: bool,
        bypass: BypassDecision,
    ) -> Vec<impl Future<Output = Result<HookOutcome, Error>> + 'cs> {
        let mut futures = Vec::new();

        match self {
            Self::Bookmark(hook, _, _) => futures.push(HookInstance::Bookmark(&**hook).run_hook(
                ctx,
                repo,
                bookmark,
                hook_name,
                scuba,
                to,
                cross_repo_push_source,
                push_authored_by,
                HookExecutionPurpose::LandAttempt,
                annotated_tags,
                maybe_pushvars,
                log_only,
                bypass,
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
        repo: &'a HookRepo,
        bookmark: &'a BookmarkKey,
        hook_name: &'cs str,
        changesets: Vec<&'cs BonsaiChangeset>,
        scuba: MononokeScubaSampleBuilder,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
        purpose: HookExecutionPurpose,
        maybe_pushvars: Option<&'a HashMap<String, Bytes>>,
        log_only: bool,
        bypass_by_cs: Arc<HashMap<ChangesetId, BypassDecision>>,
    ) -> HooksOutcome<'cs> {
        match self {
            Self::Changeset(hook, _, _) => HookInstance::Changeset(&**hook)
                .run_changeset_hook_on_many_changesets(
                    ctx,
                    repo,
                    bookmark,
                    hook_name,
                    scuba,
                    changesets,
                    cross_repo_push_source,
                    push_authored_by,
                    purpose,
                    maybe_pushvars,
                    log_only,
                    bypass_by_cs,
                ),
            Self::File(hook, _, _) => HooksOutcome::Individual(
                changesets
                    .iter()
                    .flat_map(|cs| {
                        cloned!(mut scuba);
                        // One changeset's decision is reused across all of its
                        // per-path file-hook rows.
                        let bypass = bypass_by_cs
                            .get(&cs.get_changeset_id())
                            .cloned()
                            .unwrap_or_default();
                        cs.simplified_file_changes()
                            .map(move |(path, change)| {
                                HookInstance::File(&**hook, path, change)
                                    .run_hook(
                                        ctx,
                                        repo,
                                        bookmark,
                                        hook_name,
                                        scuba.clone(),
                                        cs,
                                        cross_repo_push_source,
                                        push_authored_by,
                                        purpose,
                                        None,
                                        maybe_pushvars,
                                        log_only,
                                        bypass.clone(),
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
        let condition = HookBypassCondition::commit_message("@mybypass");

        let r = condition_trigger_reason(&condition, None, Some("@notbypass"));
        assert!(r.is_none());

        let r = condition_trigger_reason(&condition, None, Some("foo @mybypass bar"));
        assert_eq!(r.as_deref(), Some("bypass string: @mybypass"));

        // Bookmark hooks have no commit message.
        let r = condition_trigger_reason(&condition, None, None);
        assert!(r.is_none());
    }

    /// A condition with several markers needs all of them.
    #[mononoke::test]
    fn test_all_markers_required() {
        let condition = HookBypassCondition {
            commit_message: Some("@mybypass".into()),
            ..HookBypassCondition::pushvar("myvar", "myvalue")
        };
        let mut pushvars = HashMap::new();

        let r = condition_trigger_reason(&condition, Some(&pushvars), Some("@mybypass"));
        assert!(r.is_none());

        pushvars.insert("myvar".into(), "myvalue".as_bytes().into());
        let r = condition_trigger_reason(&condition, Some(&pushvars), Some("no marker"));
        assert!(r.is_none());

        let r = condition_trigger_reason(&condition, Some(&pushvars), Some("@mybypass"));
        assert_eq!(
            r.as_deref(),
            Some("bypass pushvar: myvar=myvalue, bypass string: @mybypass")
        );
    }

    /// A condition with no markers is always triggered; its other requirements
    /// decide whether it matches.
    #[mononoke::test]
    fn test_condition_without_markers() {
        let condition = HookBypassCondition::permission_group("group");
        let r = condition_trigger_reason(&condition, None, None);
        assert_eq!(r.as_deref(), Some("implicit: member of group"));

        let condition =
            HookBypassCondition::identities([MononokeIdentity::from_legacy_type_data("USER", "a")]);
        let r = condition_trigger_reason(&condition, None, None);
        assert_eq!(r.as_deref(), Some("implicit: allowed identity"));
    }

    #[mononoke::test]
    fn test_extract_identity_from_author() {
        // Service-bot authors carry their FBID in the "noreply+<FBID>" local-part
        // and resolve to an FBID identity (the account added to bypass groups).
        assert_eq!(
            extract_identity_from_author(
                "CleanupArcCallConduit Bot <noreply+2796533067383049@fb.com>",
            ),
            Some(MononokeIdentity::from_legacy_type_data(
                "FBID",
                "2796533067383049"
            )),
        );
        assert_eq!(
            extract_identity_from_author("Bot <noreply+123@meta.com>"),
            Some(MononokeIdentity::from_legacy_type_data("FBID", "123")),
        );

        // Human authors keep resolving to a USER identity from the unixname.
        assert_eq!(
            extract_identity_from_author("Jane Doe <jdoe@fb.com>"),
            Some(MononokeIdentity::from_legacy_type_data("USER", "jdoe")),
        );

        // A non-numeric "noreply+" suffix is not an FBID; fall back to USER.
        assert_eq!(
            extract_identity_from_author("Bot <noreply+abc@fb.com>"),
            Some(MononokeIdentity::from_legacy_type_data(
                "USER",
                "noreply+abc"
            )),
        );

        // The FBID shape is only trusted on internal domains.
        assert_eq!(
            extract_identity_from_author("Bot <noreply+123@external.com>"),
            Some(MononokeIdentity::from_legacy_type_data(
                "USER",
                "noreply+123"
            )),
        );

        // An unparsable author yields no identity (caller falls back to pusher).
        assert_eq!(extract_identity_from_author("no-email-author"), None);
    }

    #[mononoke::test]
    fn test_pushvar_bypass() {
        let condition = HookBypassCondition::pushvar("myvar", "myvalue");

        let mut m = HashMap::new();
        let r = condition_trigger_reason(&condition, Some(&m), None);
        assert!(r.is_none()); // No var

        m.insert("somevar".into(), "somevalue".as_bytes().into());
        let r = condition_trigger_reason(&condition, Some(&m), None);
        assert!(r.is_none()); // wrong var

        m.insert("myvar".into(), "somevalue".as_bytes().into());
        let r = condition_trigger_reason(&condition, Some(&m), None);
        assert!(r.is_none()); // wrong value

        m.insert("myvar".into(), "myvalue foo".as_bytes().into());
        let r = condition_trigger_reason(&condition, Some(&m), None);
        assert!(r.is_none()); // wrong value

        m.insert("myvar".into(), "myvalue".as_bytes().into());
        let r = condition_trigger_reason(&condition, Some(&m), None);
        assert_eq!(r.as_deref(), Some("bypass pushvar: myvar=myvalue"));
    }
}
