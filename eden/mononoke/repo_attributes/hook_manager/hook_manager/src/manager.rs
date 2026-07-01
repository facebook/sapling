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
use metaconfig_types::HookBypass;
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
use crate::ChangesetHook;
use crate::CrossRepoPushSource;
use crate::FileHook;
use crate::HookExecution;
use crate::HookOutcome;
use crate::HookRepo;
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

enum BypassAuthorizationResult {
    /// No bypass was attempted — run the hook normally.
    NoBypass,
    /// Bypass was attempted and authorized — skip the hook.
    Bypassed(String),
    /// Bypass was attempted but the user is not in the required group.
    /// Contains the group name for the rejection message.
    Unauthorized(String),
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

    pub fn register_bookmark_hook(
        &mut self,
        hook_name: &str,
        hook: Box<dyn BookmarkHook>,
        config: HookConfig,
        bypass_checker: Option<ArcMembershipChecker>,
    ) {
        self.hooks.insert(
            hook_name.to_string(),
            Hook::from_bookmark(hook, config, bypass_checker),
        );
    }

    pub fn register_changeset_hook(
        &mut self,
        hook_name: &str,
        hook: Box<dyn ChangesetHook>,
        config: HookConfig,
        bypass_checker: Option<ArcMembershipChecker>,
    ) {
        self.hooks.insert(
            hook_name.to_string(),
            Hook::from_changeset(hook, config, bypass_checker),
        );
    }

    pub fn register_file_hook(
        &mut self,
        hook_name: &str,
        hook: Box<dyn FileHook>,
        config: HookConfig,
        bypass_checker: Option<ArcMembershipChecker>,
    ) {
        self.hooks.insert(
            hook_name.to_string(),
            Hook::from_file(hook, config, bypass_checker),
        );
    }

    /// Check if a bypass is authorized given the permission group restriction.
    ///
    /// When `changeset_author` is provided (e.g., "Alice <alice@fb.com>"),
    /// group membership is checked against the commit author's identity
    /// rather than the pusher's TLS cert identities.
    async fn check_bypass_authorization(
        &self,
        hook: &Hook,
        ctx: &CoreContext,
        maybe_pushvars: Option<&HashMap<String, Bytes>>,
        cs_msg: Option<&str>,
        changeset_author: Option<&str>,
    ) -> Result<BypassAuthorizationResult> {
        let bypass = hook.get_config().bypass.as_ref();

        // First check if there's a pushvar bypass
        let bypass_reason = get_bypassed_by_pushvar_reason(bypass, maybe_pushvars)
            .or_else(|| cs_msg.and_then(|msg| get_bypassed_by_commit_msg_reason(bypass, msg)));

        let bypass_reason = match bypass_reason {
            Some(reason) => reason,
            None => return Ok(BypassAuthorizationResult::NoBypass),
        };

        // Check JustKnob — if disabled, allow bypass without group check
        let jk_enabled = justknobs::eval(
            "scm/mononoke:enable_hook_bypass_permission_groups",
            None,
            Some(self.repo_name.as_str()),
        );
        if !jk_enabled {
            return Ok(BypassAuthorizationResult::Bypassed(bypass_reason));
        }

        let use_client_identities = justknobs::eval(
            "scm/mononoke:check_hook_bypass_permission_group_with_client_identities",
            None,
            Some(self.repo_name.as_str()),
        );
        if !use_client_identities {
            return self
                .check_bypass_authorization_with_changeset_author(
                    hook,
                    ctx,
                    changeset_author,
                    bypass_reason,
                )
                .await;
        }

        // Check the permission group against the pusher's client identities.
        // Missing identities fail closed (the hook runs).
        self.check_membership(hook, ctx.metadata().identities(), bypass_reason)
            .await
    }

    /// Check whether `identity_set` is a member of the hook's bypass permission
    /// group and map the result to a BypassAuthorizationResult.
    ///
    /// If the hook has no bypass permission checker configured, the bypass is
    /// allowed unconditionally (preserves pre-permission-group behavior). An
    /// empty `identity_set` fails closed: a real membership checker returns
    /// `false`, so the result is `Unauthorized` and the hook runs normally.
    async fn check_membership(
        &self,
        hook: &Hook,
        identity_set: &MononokeIdentitySet,
        bypass_reason: String,
    ) -> Result<BypassAuthorizationResult> {
        let checker = match hook.get_bypass_permission_checker() {
            Some(checker) => checker,
            None => return Ok(BypassAuthorizationResult::Bypassed(bypass_reason)),
        };

        if checker.is_member(identity_set).await {
            Ok(BypassAuthorizationResult::Bypassed(bypass_reason))
        } else {
            let group_name = hook.get_bypass_permission_group().unwrap_or("unknown");
            Ok(BypassAuthorizationResult::Unauthorized(
                group_name.to_string(),
            ))
        }
    }

    /// Check the permission group against the commit author's identity, falling
    /// back to the pusher's client identities when the author is missing or
    /// unparsable. When the author's `USER:<local-part>` identity is not in the
    /// group, resolve their canonical unixname via the EmployeeService (JK-gated)
    /// and check that instead -- this handles authors whose email local-part
    /// differs from their unixname. The re-check can only ever upgrade
    /// `Unauthorized` -> `Bypassed`; on any miss or error the original decision
    /// stands.
    async fn check_bypass_authorization_with_changeset_author(
        &self,
        hook: &Hook,
        ctx: &CoreContext,
        changeset_author: Option<&str>,
        bypass_reason: String,
    ) -> Result<BypassAuthorizationResult> {
        let resolve_bot_fbid = justknobs::eval(
            "scm/mononoke:resolve_bot_fbid_author_for_hook_bypass",
            None,
            Some(self.repo_name.as_str()),
        );
        let author_identity = match changeset_author
            .and_then(|author| extract_identity_from_author(author, resolve_bot_fbid))
        {
            Some(author_identity) => author_identity,
            None => {
                return self
                    .check_membership(hook, ctx.metadata().identities(), bypass_reason)
                    .await;
            }
        };

        let is_user = author_identity.id_type() == "USER";
        let result = self
            .check_membership(
                hook,
                &std::iter::once(author_identity).collect(),
                bypass_reason.clone(),
            )
            .await?;

        if matches!(result, BypassAuthorizationResult::Unauthorized(_))
            && is_user
            && justknobs::eval(
                "scm/mononoke:resolve_unixname_from_employee_service_for_hook_bypass",
                None,
                Some(self.repo_name.as_str()),
            )
        {
            if let Some(unixname) = self.resolve_author_unixname(changeset_author).await {
                let identity = MononokeIdentity::from_legacy_type_data("USER", &unixname);
                return self
                    .check_membership(hook, &std::iter::once(identity).collect(), bypass_reason)
                    .await;
            }
        }
        Ok(result)
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

        for hook_name in hooks {
            let hook = self
                .hooks
                .get(hook_name)
                .ok_or_else(|| HookManagerError::NoSuchHook(hook_name.to_string()))?;

            let mut scuba = scuba.clone();
            scuba.add("hook", hook_name.to_string());
            scuba.add("to", to.get_changeset_id().to_string());

            // A bypass with no permission group is honored without running the hook
            // (bookmark hooks honor the pushvar bypass only). Group-gated bypasses
            // run and are resolved afterwards in `apply_bypasses`.
            if let Some(bypass_reason) = unconditional_bypass_reason(hook, maybe_pushvars, None) {
                scuba.add("bypass_reason", bypass_reason);
                scuba.log();
                continue;
            }

            for future in hook.get_futures_for_bookmark_hooks(
                ctx,
                &self.repo,
                bookmark,
                hook_name,
                to,
                scuba,
                cross_repo_push_source,
                push_authored_by,
                hook.get_config().log_only,
            ) {
                futs.push(future.boxed());
            }
        }
        let outcomes: Vec<HookOutcome> = futs.try_collect().await?;
        self.apply_bypasses(
            ctx,
            outcomes,
            std::slice::from_ref(to),
            maybe_pushvars,
            &scuba,
            false,
        )
        .await
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

        if let Some(user) = user_option {
            scuba.add("user", user);
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

        // Skip, before running, any (hook, changeset) bypassed unconditionally (a
        // pushvar/commit-message bypass with no permission group). Group-gated
        // bypasses run and are resolved afterwards in `apply_bypasses`.
        let hooks_with_changesets: Vec<_> = resolved_hooks
            .into_iter()
            .map(|(hook_name, hook)| {
                let changesets: Vec<&BonsaiChangeset> = changesets
                    .iter()
                    .filter(|cs| {
                        match unconditional_bypass_reason(hook, maybe_pushvars, Some(cs.message()))
                        {
                            Some(reason) => {
                                log_bypassed_changeset(&scuba, cs, &reason, None);
                                false
                            }
                            None => true,
                        }
                    })
                    .collect();
                (hook_name, hook, changesets)
            })
            .collect();

        let (batched, individual) = hooks_with_changesets
            .into_iter()
            .map(|(hook_name, hook, changesets)| {
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
                    hook.get_config().log_only,
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
        self.apply_bypasses(ctx, outcomes, changesets, maybe_pushvars, &scuba, true)
            .await
    }

    /// Apply bypasses to hook rejections via `check_bypass_authorization`. The
    /// permission group is consulted only here, so a push whose hooks pass never
    /// hits the group: an authorized (or group-less) bypass drops the rejection, an
    /// unauthorized one keeps the hook's own rejection, annotated with a note.
    async fn apply_bypasses(
        &self,
        ctx: &CoreContext,
        outcomes: Vec<HookOutcome>,
        changesets: &[BonsaiChangeset],
        maybe_pushvars: Option<&HashMap<String, Bytes>>,
        scuba: &MononokeScubaSampleBuilder,
        // Bookmark hooks only honor pushvar/author-group bypasses, not the commit
        // message; pass `false` to keep that narrower surface.
        use_commit_message: bool,
    ) -> Result<Vec<HookOutcome>> {
        let cs_by_id: HashMap<ChangesetId, &BonsaiChangeset> = changesets
            .iter()
            .map(|cs| (cs.get_changeset_id(), cs))
            .collect();

        // Resolve each (hook, changeset) at most once: a file hook can reject many
        // paths of one changeset and the membership check is an ACL RPC.
        let mut checked: HashMap<(String, ChangesetId), BypassAuthorizationResult> = HashMap::new();
        let mut result = Vec::with_capacity(outcomes.len());
        for outcome in outcomes {
            if !outcome.is_rejection() {
                result.push(outcome);
                continue;
            }
            let key = (
                outcome.get_hook_name().to_string(),
                outcome.get_changeset_id(),
            );
            // First rejection seen for this (hook, changeset): resolve the bypass
            // (one ACL check), log a honored bypass, and emit any unauthorized
            // rejection just once even if the hook rejects many paths.
            let first_rejection = !checked.contains_key(&key);
            if first_rejection {
                let hook = self
                    .hooks
                    .get(&key.0)
                    .ok_or_else(|| HookManagerError::NoSuchHook(key.0.clone()))?;
                let cs = cs_by_id.get(&key.1).copied();
                let cs_msg = if use_commit_message {
                    cs.map(|cs| cs.message())
                } else {
                    None
                };
                let decision = self
                    .check_bypass_authorization(
                        hook,
                        ctx,
                        maybe_pushvars,
                        cs_msg,
                        cs.map(|cs| cs.author()),
                    )
                    .await?;
                if let (BypassAuthorizationResult::Bypassed(reason), Some(cs)) = (&decision, cs) {
                    let mut scuba = scuba.clone();
                    scuba.add("hook", key.0.clone());
                    log_bypassed_changeset(&scuba, cs, reason, hook.get_bypass_permission_group());
                }
                checked.insert(key.clone(), decision);
            }
            match checked.get(&key) {
                Some(BypassAuthorizationResult::Bypassed(_)) => {}
                Some(BypassAuthorizationResult::Unauthorized(group)) => {
                    if first_rejection {
                        result.push(annotate_unauthorized_rejection(outcome, group));
                    }
                }
                _ => result.push(outcome),
            }
        }
        Ok(result)
    }
}

/// Append a note to a rejection telling the pusher their bypass was ignored
/// because they are not in the permission group. The hook's own reason is kept.
fn annotate_unauthorized_rejection(mut outcome: HookOutcome, group_name: &str) -> HookOutcome {
    if let Some(info) = outcome.get_execution().rejection_info() {
        let mut info = info.clone();
        info.long_description = format!(
            "{}\n\nNote: your hook bypass was ignored because you are not a member of \
             group '{group_name}'. Request access to the group, or fix the issue above.",
            info.long_description,
        );
        let extra_logs = ["Unauthorized bypass rejected".to_string()]
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
/// With `resolve_bot_fbid`, "noreply+<FBID>@fb.com" bot authors yield an FBID
/// identity; everything else yields USER:<local-part>.
fn extract_identity_from_author(author: &str, resolve_bot_fbid: bool) -> Option<MononokeIdentity> {
    let caps = AUTHOR_RE.captures(author)?;
    let local_part = caps.get(1)?.as_str();
    let host = caps.get(2)?.as_str();

    let is_internal = matches!(host.to_ascii_lowercase().as_str(), "fb.com" | "meta.com");
    // Codemod bots are Meiosis service users with no unixname; their FBID only
    // reaches us via the "noreply+<FBID>" author local-part, so recover it here.
    let fbid = if resolve_bot_fbid {
        local_part
            .strip_prefix("noreply+")
            .filter(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
    } else {
        None
    };

    Some(match fbid {
        Some(fbid) if is_internal => MononokeIdentity::from_legacy_type_data("FBID", fbid),
        _ => MononokeIdentity::from_legacy_type_data("USER", local_part),
    })
}

fn log_bypassed_changeset(
    scuba: &MononokeScubaSampleBuilder,
    cs: &BonsaiChangeset,
    bypass_reason: &str,
    bypass_permission_group: Option<&str>,
) {
    cloned!(mut scuba);
    scuba.add("hash", cs.get_changeset_id().to_string());
    scuba.add("bypass_reason", bypass_reason.to_string());
    if let Some(group) = bypass_permission_group {
        scuba.add("bypass_permission_group", group.to_string());
    }
    scuba.log();
}

/// Reason a hook is bypassed unconditionally (pushvar or commit-message bypass with
/// no permission group), so it can be skipped before it runs. Group-gated bypasses
/// return `None` here -- they need the hook's result and are resolved afterwards in
/// `apply_bypasses`. Skipping pre-run is also what keeps a bypass able to rescue a
/// hook that would otherwise error (its `Err` would abort the push before bypass
/// resolution).
fn unconditional_bypass_reason(
    hook: &Hook,
    maybe_pushvars: Option<&HashMap<String, Bytes>>,
    cs_msg: Option<&str>,
) -> Option<String> {
    if hook.get_bypass_permission_group().is_some() {
        return None;
    }
    let bypass = hook.get_config().bypass.as_ref();
    get_bypassed_by_pushvar_reason(bypass, maybe_pushvars)
        .or_else(|| cs_msg.and_then(|msg| get_bypassed_by_commit_msg_reason(bypass, msg)))
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
                    return Some(format!("bypass pushvar: {name}={value}"));
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
            return Some(format!("bypass string: {bypass_string}"));
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
    Bookmark(
        Box<dyn BookmarkHook>,
        HookConfig,
        Option<ArcMembershipChecker>,
    ),
    Changeset(
        Box<dyn ChangesetHook>,
        HookConfig,
        Option<ArcMembershipChecker>,
    ),
    File(Box<dyn FileHook>, HookConfig, Option<ArcMembershipChecker>),
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
                repo,
                bookmark,
                changesets,
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
        repo: &HookRepo,
        bookmark: &BookmarkKey,
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
                repo,
                bookmark,
                cs,
                cross_repo_push_source,
                push_authored_by,
                hook_name,
                scuba,
                log_only,
            ),
            Self::Changeset(hook) => hook.run_hook(
                ctx,
                repo,
                bookmark,
                cs,
                cross_repo_push_source,
                push_authored_by,
                hook_name,
                scuba,
                log_only,
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
            ),
        }
        .await
    }
}

impl Hook {
    pub fn from_bookmark(
        hook: Box<dyn BookmarkHook>,
        config: HookConfig,
        bypass_checker: Option<ArcMembershipChecker>,
    ) -> Self {
        Self::Bookmark(hook, config, bypass_checker)
    }

    pub fn from_changeset(
        hook: Box<dyn ChangesetHook>,
        config: HookConfig,
        bypass_checker: Option<ArcMembershipChecker>,
    ) -> Self {
        Self::Changeset(hook, config, bypass_checker)
    }

    pub fn from_file(
        hook: Box<dyn FileHook>,
        config: HookConfig,
        bypass_checker: Option<ArcMembershipChecker>,
    ) -> Self {
        Self::File(hook, config, bypass_checker)
    }

    pub fn get_config(&self) -> &HookConfig {
        match self {
            Self::Bookmark(_, config, _) => config,
            Self::Changeset(_, config, _) => config,
            Self::File(_, config, _) => config,
        }
    }

    pub fn get_bypass_permission_checker(&self) -> Option<&ArcMembershipChecker> {
        match self {
            Self::Bookmark(_, _, checker)
            | Self::Changeset(_, _, checker)
            | Self::File(_, _, checker) => checker.as_ref(),
        }
    }

    /// The permission group that restricts who is allowed to bypass this hook,
    /// if one is configured. Logged whenever a bypass succeeds so we can tell
    /// when a bypass was performed by a member of the restricting group.
    pub(crate) fn get_bypass_permission_group(&self) -> Option<&str> {
        self.get_config()
            .bypass
            .as_ref()
            .and_then(|bypass| bypass.permission_group())
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
        log_only: bool,
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
        repo: &'a HookRepo,
        bookmark: &'a BookmarkKey,
        hook_name: &'cs str,
        changesets: Vec<&'cs BonsaiChangeset>,
        scuba: MononokeScubaSampleBuilder,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
        log_only: bool,
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
                    log_only,
                ),
            Self::File(hook, _, _) => HooksOutcome::Individual(
                changesets
                    .iter()
                    .flat_map(|cs| {
                        cloned!(mut scuba);
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
    fn test_extract_identity_from_author() {
        // Service-bot authors carry their FBID in the "noreply+<FBID>" local-part
        // and, with the killswitch on, resolve to an FBID identity (the account
        // added to bypass groups).
        assert_eq!(
            extract_identity_from_author(
                "CleanupArcCallConduit Bot <noreply+2796533067383049@fb.com>",
                true,
            ),
            Some(MononokeIdentity::from_legacy_type_data(
                "FBID",
                "2796533067383049"
            )),
        );
        assert_eq!(
            extract_identity_from_author("Bot <noreply+123@meta.com>", true),
            Some(MononokeIdentity::from_legacy_type_data("FBID", "123")),
        );

        // Human authors keep resolving to a USER identity from the unixname.
        assert_eq!(
            extract_identity_from_author("Jane Doe <jdoe@fb.com>", true),
            Some(MononokeIdentity::from_legacy_type_data("USER", "jdoe")),
        );

        // A non-numeric "noreply+" suffix is not an FBID; fall back to USER.
        assert_eq!(
            extract_identity_from_author("Bot <noreply+abc@fb.com>", true),
            Some(MononokeIdentity::from_legacy_type_data(
                "USER",
                "noreply+abc"
            )),
        );

        // The FBID shape is only trusted on internal domains.
        assert_eq!(
            extract_identity_from_author("Bot <noreply+123@external.com>", true),
            Some(MononokeIdentity::from_legacy_type_data(
                "USER",
                "noreply+123"
            )),
        );

        // An unparsable author yields no identity (caller falls back to pusher).
        assert_eq!(extract_identity_from_author("no-email-author", true), None);
    }

    #[mononoke::test]
    fn test_extract_identity_from_author_killswitch_off() {
        // With the killswitch off, the bot author keeps its pre-rollout (junk) USER
        // identity from the whole local-part instead of resolving to its FBID.
        assert_eq!(
            extract_identity_from_author(
                "CleanupArcCallConduit Bot <noreply+2796533067383049@fb.com>",
                false,
            ),
            Some(MononokeIdentity::from_legacy_type_data(
                "USER",
                "noreply+2796533067383049"
            )),
        );

        // Human authors are unaffected by the killswitch.
        assert_eq!(
            extract_identity_from_author("Jane Doe <jdoe@fb.com>", false),
            Some(MononokeIdentity::from_legacy_type_data("USER", "jdoe")),
        );
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
