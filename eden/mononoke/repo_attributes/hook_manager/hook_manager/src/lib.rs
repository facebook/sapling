/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Hooks are sets of constraints that can be applied to commits when they
//! become ancestors of a particular public bookmark.  The hook manager
//! ensures that commits meet the constraints that the hooks require.

pub mod errors;
pub mod manager;
pub mod repo;
#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;
use std::str;
use std::sync::Arc;

use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use bookmarks_types::AnnotatedTags;
use bookmarks_types::BookmarkKey;
use bytes::Bytes;
use context::CoreContext;
use futures::FutureExt;
use futures::TryFutureExt;
use futures_stats::FutureStats;
use futures_stats::TimedFutureExt;
use mononoke_types::BasicFileChange;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::NonRootMPath;
use mononoke_types::hash::GitSha1;
use permission_checker::MononokeIdentitySet;
use scuba::ScubaValue;
use scuba_ext::MononokeScubaSampleBuilder;
use strum::IntoStaticStr;

pub use crate::errors::HookManagerError;
pub use crate::manager::HookManager;
use crate::manager::HooksOutcome;
use crate::manager::annotate_agent_bypass_rejection;
use crate::manager::annotate_unauthorized_rejection;
pub use crate::repo::HookRepo;

/// Pushvars the client supplied on this push (`hg push --pushvar KEY=VALUE`).
///
/// Client-controlled and carrying no authentication of their own, so a hook
/// must never treat one as authorization without also checking the pusher's
/// identity.
pub type Pushvars = HashMap<String, Bytes>;

/// Whether changesets were created by a user or a service.
///
/// If it is a service then most hooks should just exit with a success because
/// we trust service writes. However, some hooks like verify_integrity might
/// still need to do some checks and/or logging.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PushAuthoredBy {
    User,
    Service,
}

impl PushAuthoredBy {
    /// True if this push was authored by a service.
    pub fn service(&self) -> bool {
        *self == PushAuthoredBy::Service
    }
}

/// The origin of the changeset.
///
/// In the push-redirection scenario the changeset is initially pushed to a
/// small repo and then redirected to a large one. An opposite of this is a
/// changeset, native to the large repo, which does not go through the
/// push-redirection.  We want hooks to be able to distinguish the two.
///
/// Note: this functionality is rarely needed. You should always strive to
/// write hooks that ignore this information.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrossRepoPushSource {
    /// Changeset pushed directly to the large repo
    NativeToThisRepo,
    /// Changeset push-redirected from the small repo
    PushRedirected,
}

/// Enum describing the state of a bookmark for which hooks are being run.
pub enum BookmarkState {
    /// The bookmark is new and is being created by the current push
    New,
    /// The bookmark is existing and is being moved by the current push
    Existing(ChangesetId),
    // No Deleted state because hooks are not run on deleted bookmarks
}

impl BookmarkState {
    pub fn is_new(&self) -> bool {
        if let BookmarkState::New = *self {
            return true;
        }
        false
    }

    pub fn is_existing(&self) -> bool {
        !self.is_new()
    }
}

#[derive(Clone, Debug)]
pub enum PathContent {
    Directory,
    File(ContentId),
}

#[derive(Clone, Debug)]
pub enum FileChangeType {
    Added(ContentId),
    Changed(ContentId, ContentId),
    Removed,
}

/// Enum describing the type of a tag for which hooks are being run.
pub enum TagType {
    /// The bookmark is not a tag at all
    NotATag,
    /// The bookmark is a simple tag with no object associated with it
    LightweightTag,
    /// The bookmark is an annotated tag with an associated object with GitSha1 hash
    AnnotatedTag(GitSha1),
}

/// Where the identity set tested against a hook's bypass permission group came
/// from. A denied bypass is otherwise ambiguous: the group name alone does not
/// say whose membership was actually checked, and the commit-author path can
/// silently fall back to the pusher.
///
/// Up to two checks can run for one decision (client identities, then the commit
/// author). This records the one that *decided*, so a client-identity miss that
/// falls through reports the author check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BypassIdentitySource {
    /// The pusher's own request identities.
    ClientIdentities,
    /// An identity derived from the commit author.
    CommitAuthor,
    /// The commit author was absent or unparsable, so the pusher's request
    /// identities were tested instead. These are usually *not* the author's.
    CommitAuthorFallbackToClient,
    /// The author's canonical unixname, resolved via the EmployeeService after
    /// the author's own identity missed.
    CommitAuthorResolvedUnixname,
}

impl BypassIdentitySource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ClientIdentities => "client_identities",
            Self::CommitAuthor => "commit_author",
            Self::CommitAuthorFallbackToClient => "commit_author_fallback_to_client",
            Self::CommitAuthorResolvedUnixname => "commit_author_resolved_unixname",
        }
    }
}

/// The identity set a bypass permission-group check ran against, and where it
/// came from. Logged to Scuba for debugging; deliberately not surfaced in the
/// pusher-facing rejection message.
///
/// `identities` holds `TYPE:data` strings only, never typed/CAT-bearing
/// renderings.
#[derive(Clone, Debug)]
pub struct CheckedBypassIdentities {
    pub source: BypassIdentitySource,
    pub identities: Vec<String>,
}

impl CheckedBypassIdentities {
    pub fn new(source: BypassIdentitySource, identities: &MononokeIdentitySet) -> Self {
        Self {
            source,
            identities: identities.iter().map(|id| id.to_string()).collect(),
        }
    }
}

/// A bypass decision computed eagerly (before the hook runs) for a single
/// `(hook, changeset)` pair, and carried into the hook-run path.
///
/// The reason a bypass fired and the hook's permission-group name are bundled
/// here because the trait side (`run_hook`) cannot see the `Hook` enum that
/// holds the config.
#[derive(Clone, Debug, Default, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum BypassDecision {
    /// No bypass was attempted (or none applies) — the hook's own result stands.
    #[default]
    #[strum(serialize = "none")]
    NoBypass,
    /// A bypass fired and the pusher is authorized — the rejection is folded
    /// into a single `accepted_via_bypass` row. Carries the bypass reason and
    /// the restricting permission group (if any).
    Authorized {
        reason: String,
        permission_group: Option<String>,
        check: Option<CheckedBypassIdentities>,
    },
    /// A bypass fired but the pusher is not in the required group — the
    /// rejection stands, annotated with a "not a member" note.
    UnauthorizedUser {
        reason: String,
        group: String,
        check: CheckedBypassIdentities,
    },
    /// A bypass fired and the pusher is in the required group, but the pusher
    /// is an agent — the rejection stands, annotated with a note handing the
    /// decision back to a human. Carries the bypass reason and the restricting
    /// permission group.
    UnauthorizedAgent {
        reason: String,
        group: String,
        check: Option<CheckedBypassIdentities>,
    },
}

impl BypassDecision {
    /// The decision's name as logged in the `log_only_bypass_decision` column.
    /// Backed by the `IntoStaticStr` derive.
    pub fn as_str(&self) -> &'static str {
        self.into()
    }
}

/// Add the mechanical execution-stats columns to `scuba`: common server data,
/// request metadata, perf counters, any `extra_logs` the hook produced, and the
/// elapsed timing. This does not interpret the hook's verdict and does not log;
/// the outcome columns and the final `.log()` are handled by the caller via
/// `record_outcome_and_apply_bypass`.
fn log_execution_stats(
    ctx: &CoreContext,
    scuba: &mut MononokeScubaSampleBuilder,
    stats: FutureStats,
    result: &Result<HookOutcome>,
) {
    scuba.add_common_server_data();
    scuba.add_metadata(ctx.metadata());
    ctx.perf_counters().insert_perf_counters(scuba);

    // Emit the `extra_logs` column (a normvector) when the hook produced any.
    if let Ok(outcome) = result.as_ref() {
        if let Some(v) = extra_logs_scuba_value(&outcome.get_execution().extra_logs) {
            scuba.add("extra_logs", v);
        }
    }

    let elapsed = stats.completion_time.as_millis() as i64;
    scuba.add("elapsed", elapsed).add("total_time", elapsed);
}

/// Add the `bypass_*` columns describing `bypass` to `scuba`; `NoBypass` adds
/// none. Shared by the enforcing and log-only paths of
/// `record_outcome_and_apply_bypass` so the columns mean the same thing in both.
///
/// Denials record the columns too, so a denied bypass is distinguishable from a
/// rejection where none was attempted: `bypass_reason IS NOT NULL` selects
/// exactly the attempts. `bypass_blocked_for_agent` is what separates the agent
/// case from a plain "not a member" denial.
fn add_bypass_decision_columns(scuba: &mut MononokeScubaSampleBuilder, bypass: &BypassDecision) {
    match bypass {
        BypassDecision::NoBypass => {}
        BypassDecision::Authorized {
            reason,
            permission_group,
            check,
        } => {
            scuba.add("bypass_reason", reason.clone());
            if let Some(group) = permission_group {
                scuba.add("bypass_permission_group", group.clone());
            }
            if let Some(check) = check {
                scuba
                    .add("bypass_identity_source", check.source.as_str())
                    .add("bypass_identities_checked", check.identities.clone());
            }
        }
        BypassDecision::UnauthorizedUser {
            reason,
            group,
            check,
        } => {
            scuba
                .add("bypass_reason", reason.clone())
                .add("bypass_permission_group", group.clone())
                .add("bypass_identity_source", check.source.as_str())
                .add("bypass_identities_checked", check.identities.clone());
        }
        BypassDecision::UnauthorizedAgent {
            reason,
            group,
            check,
        } => {
            scuba
                .add("bypass_reason", reason.clone())
                .add("bypass_permission_group", group.clone())
                .add("bypass_blocked_for_agent", true);
            if let Some(check) = check {
                scuba
                    .add("bypass_identity_source", check.source.as_str())
                    .add("bypass_identities_checked", check.identities.clone());
            }
        }
    }
}

/// Record, without applying it, the bypass decision that would have applied to
/// a log-only rejection: `log_only_bypass_decision` names the decision and the
/// usual `bypass_*` columns carry its details, so a log-only rollout shows
/// whether a pusher's bypass would be honored once the hook enforces.
fn add_log_only_bypass_columns(scuba: &mut MononokeScubaSampleBuilder, bypass: &BypassDecision) {
    scuba.add("log_only_bypass_decision", bypass.as_str());
    add_bypass_decision_columns(scuba, bypass);
}

/// Set the outcome columns (`outcome`, `errorcode`, `failed_hooks`, `stderr`, and
/// any `bypass_*`) on `scuba` and return the finalized `HookOutcome`; the caller
/// logs afterwards. The bypass decision is consulted exactly once, here. When
/// enforcing, an authorized bypass folds a rejection into an accepted
/// `accepted_via_bypass` row and an unauthorized one keeps the rejection with a
/// "not a member" note. When log-only, the rejection is accepted regardless and
/// the decision is only recorded (`log_only_bypass_decision`), never applied.
fn record_outcome_and_apply_bypass(
    scuba: &mut MononokeScubaSampleBuilder,
    result: Result<HookOutcome>,
    log_only: bool,
    bypass: &BypassDecision,
) -> Result<HookOutcome> {
    let mut outcome = match result {
        Err(e) => {
            scuba
                .add("internal_failure", true)
                .add("stderr", format!("{e:?}"))
                .add("errorcode", 1)
                .add("failed_hooks", 0)
                .add("outcome", "error");
            return Err(e);
        }
        Ok(outcome) => outcome,
    };

    if !outcome.get_execution().is_rejected() {
        scuba
            .add("errorcode", 0)
            .add("failed_hooks", 0)
            .add("outcome", "accepted");
        return Ok(outcome);
    }

    let long_description = outcome
        .get_execution()
        .rejection_info()
        .map(|info| info.long_description.clone())
        .unwrap_or_default();

    if log_only {
        // Only logging: preserve any `extra_logs` but turn the rejection into an
        // accepted execution so the push is not blocked. The bypass decision is
        // not applied (there is nothing to bypass), but it is recorded so the
        // rollout shows who would and would not get through once enforcing.
        scuba
            .add("log_only_rejection", long_description)
            .add("errorcode", 0)
            .add("failed_hooks", 0)
            .add("outcome", "log_only_rejected");
        add_log_only_bypass_columns(scuba, bypass);
        let extra_logs = outcome.get_execution().extra_logs.clone();
        outcome.set_execution(HookExecution::accepted_with_logs(extra_logs));
        return Ok(outcome);
    }

    add_bypass_decision_columns(scuba, bypass);
    match bypass {
        // An authorized bypass folds the rejection into a single
        // `accepted_via_bypass` row: it did not block the push, so it is not
        // counted as a failure.
        BypassDecision::Authorized { .. } => {
            scuba
                .add("errorcode", 0)
                .add("failed_hooks", 0)
                .add("outcome", "accepted_via_bypass");
            let extra_logs = outcome.get_execution().extra_logs.clone();
            outcome.set_execution(HookExecution::accepted_with_logs(extra_logs));
            Ok(outcome)
        }
        BypassDecision::NoBypass => {
            scuba
                .add("stderr", long_description)
                .add("errorcode", 1)
                .add("failed_hooks", 1)
                .add("outcome", "rejected");
            Ok(outcome)
        }
        BypassDecision::UnauthorizedUser { group, .. } => {
            scuba
                .add("stderr", long_description)
                .add("errorcode", 1)
                .add("failed_hooks", 1)
                .add("outcome", "rejected");
            Ok(annotate_unauthorized_rejection(outcome, group))
        }
        BypassDecision::UnauthorizedAgent { group, .. } => {
            scuba
                .add("stderr", long_description)
                .add("errorcode", 1)
                .add("failed_hooks", 1)
                .add("outcome", "rejected");
            Ok(annotate_agent_bypass_rejection(outcome, group))
        }
    }
}

/// Trait to be implemented by bookmarks hooks.
///
/// Changeset hooks run once per bookmark movement, and primarily concern themselves
/// with bookmarks metadata.
#[async_trait]
pub trait BookmarkHook: Send + Sync {
    /// `annotated_tags`: server-computed tags known annotated in this push, whose mapping row may be written only in the bookmark txn (after this hook runs); `None` if unprovided.
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'repo: 'cs>(
        &'this self,
        ctx: &'ctx CoreContext,
        repo: &'repo HookRepo,
        bookmark: &BookmarkKey,
        to: &'cs BonsaiChangeset,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
        annotated_tags: Option<&AnnotatedTags>,
    ) -> Result<HookExecution, Error>;

    async fn run_hook<'this: 'cs, 'ctx: 'this, 'cs, 'repo: 'cs>(
        &'this self,
        ctx: &'ctx CoreContext,
        repo: &'repo HookRepo,
        bookmark: &BookmarkKey,
        to: &'cs BonsaiChangeset,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
        annotated_tags: Option<&AnnotatedTags>,
        hook_name: &str,
        mut scuba: MononokeScubaSampleBuilder,
        log_only: bool,
        bypass: BypassDecision,
    ) -> Result<HookOutcome, Error> {
        let (stats, result) = self
            .run(
                ctx,
                repo,
                bookmark,
                to,
                cross_repo_push_source,
                push_authored_by,
                annotated_tags,
            )
            .map_ok(|exec| {
                HookOutcome::BookmarkHook(
                    BookmarkHookExecutionId {
                        cs_id: to.get_changeset_id(),
                        bookmark_name: bookmark.to_string(),
                        hook_name: hook_name.to_string(),
                    },
                    exec,
                )
            })
            .timed()
            .await;
        scuba.add("changeset_id", to.get_changeset_id().to_string());
        scuba.add("author", to.author().to_string());
        scuba.add("type", "bookmark");
        scuba.add("push_authored_by", format!("{push_authored_by:?}"));

        log_execution_stats(ctx, &mut scuba, stats, &result);
        let result = record_outcome_and_apply_bypass(&mut scuba, result, log_only, &bypass);
        scuba.log();
        result.map_err(|e| e.context(format!("while executing hook {hook_name}")))
    }
}

/// Trait to be implemented by changeset hooks.
///
/// Changeset hooks run once per changeset, and primarily concern themselves
/// with changeset metadata, or the overall set of modified files.
#[async_trait]
pub trait ChangesetHook: Send + Sync {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'repo: 'cs>(
        &'this self,
        ctx: &'ctx CoreContext,
        repo: &'repo HookRepo,
        bookmark: &BookmarkKey,
        changeset: &'cs BonsaiChangeset,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
        maybe_pushvars: Option<&'cs Pushvars>,
    ) -> Result<HookExecution, Error>;

    async fn run_hook<'this: 'cs, 'ctx: 'this, 'cs, 'repo: 'cs>(
        &'this self,
        ctx: &'ctx CoreContext,
        repo: &'repo HookRepo,
        bookmark: &BookmarkKey,
        changeset: &'cs BonsaiChangeset,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
        maybe_pushvars: Option<&'cs Pushvars>,
        hook_name: &str,
        mut scuba: MononokeScubaSampleBuilder,
        log_only: bool,
        bypass: BypassDecision,
    ) -> Result<HookOutcome, Error> {
        let (stats, result) = self
            .run(
                ctx,
                repo,
                bookmark,
                changeset,
                cross_repo_push_source,
                push_authored_by,
                maybe_pushvars,
            )
            .map_ok(|exec| {
                HookOutcome::ChangesetHook(
                    ChangesetHookExecutionId {
                        cs_id: changeset.get_changeset_id(),
                        hook_name: hook_name.to_string(),
                    },
                    exec,
                )
            })
            .timed()
            .await;
        // TODO: delete the hash column later
        scuba.add("hash", changeset.get_changeset_id().to_string());
        scuba.add("changeset_id", changeset.get_changeset_id().to_string());
        scuba.add("author", changeset.author().to_string());
        scuba.add("type", "changeset");
        scuba.add("push_authored_by", format!("{push_authored_by:?}"));

        log_execution_stats(ctx, &mut scuba, stats, &result);
        let result = record_outcome_and_apply_bypass(&mut scuba, result, log_only, &bypass);
        scuba.log();
        result.map_err(|e| e.context(format!("while executing hook {hook_name}")))
    }

    fn run_hook_on_many_changesets<'this: 'cs, 'ctx: 'this, 'cs, 'repo: 'cs>(
        &'this self,
        ctx: &'ctx CoreContext,
        repo: &'repo HookRepo,
        bookmark: &'cs BookmarkKey,
        changesets: Vec<&'cs BonsaiChangeset>,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
        maybe_pushvars: Option<&'cs Pushvars>,
        hook_name: &'cs str,
        scuba: MononokeScubaSampleBuilder,
        log_only: bool,
        bypass_by_cs: Arc<HashMap<ChangesetId, BypassDecision>>,
    ) -> HooksOutcome<'cs> {
        HooksOutcome::Individual(
            changesets
                .into_iter()
                .map(|cs| {
                    let bypass = bypass_by_cs
                        .get(&cs.get_changeset_id())
                        .cloned()
                        .unwrap_or_default();
                    self.run_hook(
                        ctx,
                        repo,
                        bookmark,
                        cs,
                        cross_repo_push_source,
                        push_authored_by,
                        maybe_pushvars,
                        hook_name,
                        scuba.clone(),
                        log_only,
                        bypass,
                    )
                    .boxed()
                })
                .collect(),
        )
    }
}

/// Trait to be implemented by file hooks.
///
/// File hooks run once per file change, and primarily concern themselves with
/// the file's path or contents.
#[async_trait]
pub trait FileHook: Send + Sync {
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'repo: 'change, 'path: 'change>(
        &'this self,
        ctx: &'ctx CoreContext,
        repo: &'repo HookRepo,
        change: Option<&'change BasicFileChange>,
        path: &'path NonRootMPath,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error>;

    async fn run_hook<'this: 'change, 'ctx: 'this, 'change, 'repo: 'change, 'path: 'change>(
        &'this self,
        ctx: &'ctx CoreContext,
        repo: &'repo HookRepo,
        change: Option<&'change BasicFileChange>,
        path: &'path NonRootMPath,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
        cs_id: ChangesetId,
        hook_name: &str,
        mut scuba: MononokeScubaSampleBuilder,
        log_only: bool,
        bypass: BypassDecision,
    ) -> Result<HookOutcome, Error> {
        let (stats, result) = self
            .run(
                ctx,
                repo,
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
            .await;
        scuba.add("changeset_id", cs_id.to_string());
        scuba.add("type", "file");
        log_execution_stats(ctx, &mut scuba, stats, &result);
        let result = record_outcome_and_apply_bypass(&mut scuba, result, log_only, &bypass);
        scuba.log();
        result.map_err(|e| e.context(format!("while executing hook {hook_name}")))
    }
}

/// Outcome of running a hook.
#[derive(Clone, Debug, PartialEq)]
pub enum HookOutcome {
    BookmarkHook(BookmarkHookExecutionId, HookExecution),
    ChangesetHook(ChangesetHookExecutionId, HookExecution),
    FileHook(FileHookExecutionId, HookExecution),
}

impl fmt::Display for HookOutcome {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            HookOutcome::BookmarkHook(id, exec) => {
                write!(
                    f,
                    "{} for bookmark {}, cs {}: {}",
                    id.hook_name, id.bookmark_name, id.cs_id, exec
                )
            }
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
        self.get_execution().is_rejected()
    }

    pub fn is_accept(&self) -> bool {
        !self.is_rejection()
    }

    pub fn get_hook_name(&self) -> &str {
        match self {
            HookOutcome::BookmarkHook(id, _) => &id.hook_name,
            HookOutcome::ChangesetHook(id, _) => &id.hook_name,
            HookOutcome::FileHook(id, _) => &id.hook_name,
        }
    }

    pub fn get_file_path(&self) -> Option<&NonRootMPath> {
        match self {
            HookOutcome::BookmarkHook(..) => None,
            HookOutcome::ChangesetHook(..) => None,
            HookOutcome::FileHook(id, _) => Some(&id.path),
        }
    }

    pub fn get_changeset_id(&self) -> ChangesetId {
        match self {
            HookOutcome::BookmarkHook(id, _) => id.cs_id,
            HookOutcome::ChangesetHook(id, _) => id.cs_id,
            HookOutcome::FileHook(id, _) => id.cs_id,
        }
    }

    pub fn get_execution(&self) -> &HookExecution {
        match self {
            HookOutcome::BookmarkHook(_, exec) => exec,
            HookOutcome::ChangesetHook(_, exec) => exec,
            HookOutcome::FileHook(_, exec) => exec,
        }
    }

    pub fn set_execution(&mut self, new_exec: HookExecution) {
        match self {
            HookOutcome::BookmarkHook(_, exec) => *exec = new_exec,
            HookOutcome::ChangesetHook(_, exec) => *exec = new_exec,
            HookOutcome::FileHook(_, exec) => *exec = new_exec,
        }
    }

    pub fn into_rejection(self) -> Option<HookRejection> {
        // Note: `extra_logs` are intentionally dropped here (matched via `..`).
        // They are consumed earlier by `log_execution_stats` (which runs inside
        // `run_hook` before any `into_rejection` consumer), so a `HookRejection`
        // only needs to carry the rejection reason.
        match self {
            HookOutcome::BookmarkHook(
                _,
                HookExecution {
                    result: HookResult::Accepted,
                    ..
                },
            )
            | HookOutcome::ChangesetHook(
                _,
                HookExecution {
                    result: HookResult::Accepted,
                    ..
                },
            )
            | HookOutcome::FileHook(
                _,
                HookExecution {
                    result: HookResult::Accepted,
                    ..
                },
            ) => None,
            HookOutcome::BookmarkHook(
                BookmarkHookExecutionId {
                    cs_id,
                    bookmark_name: _,
                    hook_name,
                },
                HookExecution {
                    result: HookResult::Rejected(reason),
                    ..
                },
            )
            | HookOutcome::ChangesetHook(
                ChangesetHookExecutionId { cs_id, hook_name },
                HookExecution {
                    result: HookResult::Rejected(reason),
                    ..
                },
            )
            | HookOutcome::FileHook(
                FileHookExecutionId {
                    cs_id,
                    hook_name,
                    path: _,
                },
                HookExecution {
                    result: HookResult::Rejected(reason),
                    ..
                },
            ) => Some(HookRejection {
                hook_name,
                cs_id,
                reason,
            }),
        }
    }
}

/// The rejection of a changeset by a hook.
#[derive(Clone, Debug, PartialEq)]
pub struct HookRejection {
    /// The hook that rejected the changeset.
    pub hook_name: String,

    /// The changeset that was rejected.
    pub cs_id: ChangesetId,

    /// Why the hook rejected the changeset.
    pub reason: HookRejectionInfo,
}

/// Result of executing a hook.
#[derive(Clone, Debug, PartialEq)]
pub enum HookResult {
    Accepted,
    Rejected(HookRejectionInfo),
}

/// Full outcome of one hook run: the result plus any extra diagnostic log
/// lines the hook chose to emit (surfaced in Scuba for debugging).
#[derive(Clone, Debug, PartialEq)]
pub struct HookExecution {
    pub result: HookResult,
    pub extra_logs: Vec<String>,
}

impl HookExecution {
    /// An accepted execution with no extra logs.
    pub fn accepted() -> Self {
        Self {
            result: HookResult::Accepted,
            extra_logs: vec![],
        }
    }

    /// A rejected execution with no extra logs.
    pub fn rejected(info: HookRejectionInfo) -> Self {
        Self {
            result: HookResult::Rejected(info),
            extra_logs: vec![],
        }
    }

    /// An accepted execution carrying extra diagnostic log lines.
    pub fn accepted_with_logs(extra_logs: Vec<String>) -> Self {
        Self {
            result: HookResult::Accepted,
            extra_logs,
        }
    }

    /// A rejected execution carrying extra diagnostic log lines.
    pub fn rejected_with_logs(info: HookRejectionInfo, extra_logs: Vec<String>) -> Self {
        Self {
            result: HookResult::Rejected(info),
            extra_logs,
        }
    }

    /// True if this execution accepted the changeset.
    pub fn is_accepted(&self) -> bool {
        matches!(self.result, HookResult::Accepted)
    }

    /// True if this execution rejected the changeset.
    pub fn is_rejected(&self) -> bool {
        !self.is_accepted()
    }

    /// Borrow the rejection reason, if this execution rejected.
    pub(crate) fn rejection_info(&self) -> Option<&HookRejectionInfo> {
        match &self.result {
            HookResult::Rejected(info) => Some(info),
            HookResult::Accepted => None,
        }
    }

    /// True if this execution rejected for a reason satisfying `predicate`.
    pub fn is_rejected_with_reason(
        &self,
        predicate: impl FnOnce(&HookRejectionInfo) -> bool,
    ) -> bool {
        self.rejection_info().is_some_and(predicate)
    }
}

impl From<HookOutcome> for HookExecution {
    fn from(outcome: HookOutcome) -> Self {
        match outcome {
            HookOutcome::BookmarkHook(_, r) => r,
            HookOutcome::ChangesetHook(_, r) => r,
            HookOutcome::FileHook(_, r) => r,
        }
    }
}

impl fmt::Display for HookResult {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            HookResult::Accepted => write!(f, "Accepted"),
            HookResult::Rejected(reason) => write!(f, "Rejected: {}", reason.long_description),
        }
    }
}

impl fmt::Display for HookExecution {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // `extra_logs` are intentionally not shown; user-facing output is the
        // result only.
        write!(f, "{}", self.result)
    }
}

/// Build the `extra_logs` Scuba value, or `None` when there are no logs.
/// Always a normvector (list), never a joined string.
pub(crate) fn extra_logs_scuba_value(logs: &[String]) -> Option<ScubaValue> {
    if logs.is_empty() {
        None
    } else {
        Some(ScubaValue::from(logs.to_vec()))
    }
}

/// Information on why the hook rejected the changeset
#[derive(Clone, Debug, PartialEq)]
pub struct HookRejectionInfo {
    /// A short description for summarizing this failure with similar failures
    pub description: Cow<'static, str>,
    /// A full explanation of what went wrong, suitable for presenting to the
    /// user (should include guidance for fixing this failure, where possible)
    pub long_description: String,
}

impl HookRejectionInfo {
    /// A rejection with just a short description
    ///
    /// The text should just summarize this failure - it should not be
    /// different on different invocations of this hook
    pub fn new(description: &'static str) -> Self {
        Self::new_long(description, description.to_string())
    }

    /// A rejection with a possible per-invocation fix explanation.
    pub fn new_long(
        description: &'static str,
        long_description: impl Into<Option<String>>,
    ) -> Self {
        let long_description = long_description
            .into()
            .unwrap_or_else(|| description.to_string());
        Self {
            description: Cow::Borrowed(description),
            long_description,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Hash, Eq)]
pub struct BookmarkHookExecutionId {
    pub cs_id: ChangesetId,
    pub bookmark_name: String,
    pub hook_name: String,
}

#[derive(Clone, Debug, PartialEq, Hash, Eq)]
pub struct FileHookExecutionId {
    pub cs_id: ChangesetId,
    pub hook_name: String,
    pub path: NonRootMPath,
}

#[derive(Clone, Debug, PartialEq, Hash, Eq)]
pub struct ChangesetHookExecutionId {
    pub cs_id: ChangesetId,
    pub hook_name: String,
}
