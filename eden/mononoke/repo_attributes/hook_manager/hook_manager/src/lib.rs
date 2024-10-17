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
pub mod provider;
#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::fmt;
use std::hash::Hash;
use std::str;

use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use bookmarks_types::BookmarkKey;
use context::CoreContext;
use futures::FutureExt;
use futures::TryFutureExt;
use futures_stats::FutureStats;
use futures_stats::TimedFutureExt;
use mononoke_types::BasicFileChange;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::NonRootMPath;
use scuba_ext::MononokeScubaSampleBuilder;

pub use crate::errors::HookManagerError;
pub use crate::errors::HookStateProviderError;
pub use crate::manager::HookManager;
use crate::manager::HooksOutcome;
pub use crate::provider::memory::InMemoryHookStateProvider;
pub use crate::provider::text_only::TextOnlyHookStateProvider;
pub use crate::provider::FileChangeType;
pub use crate::provider::HookStateProvider;
pub use crate::provider::PathContent;

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

fn log_execution_stats(
    mut scuba: MononokeScubaSampleBuilder,
    stats: FutureStats,
    result: &mut Result<HookOutcome>,
    log_only: bool,
) {
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
}

/// Trait to be implemented by bookmarks hooks.
///
/// Changeset hooks run once per bookmark movement, and primarily concern themselves
/// with bookmarks metadata.
#[async_trait]
pub trait BookmarkHook: Send + Sync {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'provider: 'cs>(
        &'this self,
        ctx: &'ctx CoreContext,
        bookmark: &BookmarkKey,
        to: &'cs BonsaiChangeset,
        content_provider: &'provider dyn HookStateProvider,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error>;

    async fn run_hook<'this: 'cs, 'ctx: 'this, 'cs, 'provider: 'cs>(
        &'this self,
        ctx: &'ctx CoreContext,
        bookmark: &BookmarkKey,
        to: &'cs BonsaiChangeset,
        content_provider: &'provider dyn HookStateProvider,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
        hook_name: &str,
        scuba: MononokeScubaSampleBuilder,
        log_only: bool,
    ) -> Result<HookOutcome, Error> {
        let (stats, mut result) = self
            .run(
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
                        cs_id: to.get_changeset_id(),
                        bookmark_name: bookmark.to_string(),
                        hook_name: hook_name.to_string(),
                    },
                    exec,
                )
            })
            .timed()
            .await;
        log_execution_stats(scuba, stats, &mut result, log_only);
        result.map_err(|e| e.context(format!("while executing hook {}", hook_name)))
    }
}

/// Trait to be implemented by changeset hooks.
///
/// Changeset hooks run once per changeset, and primarily concern themselves
/// with changeset metadata, or the overall set of modified files.
#[async_trait]
pub trait ChangesetHook: Send + Sync {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'provider: 'cs>(
        &'this self,
        ctx: &'ctx CoreContext,
        bookmark: &BookmarkKey,
        changeset: &'cs BonsaiChangeset,
        content_provider: &'provider dyn HookStateProvider,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error>;

    async fn run_hook<'this: 'cs, 'ctx: 'this, 'cs, 'provider: 'cs>(
        &'this self,
        ctx: &'ctx CoreContext,
        bookmark: &BookmarkKey,
        changeset: &'cs BonsaiChangeset,
        content_provider: &'provider dyn HookStateProvider,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
        hook_name: &str,
        scuba: MononokeScubaSampleBuilder,
        log_only: bool,
    ) -> Result<HookOutcome, Error> {
        let (stats, mut result) = self
            .run(
                ctx,
                bookmark,
                changeset,
                content_provider,
                cross_repo_push_source,
                push_authored_by,
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
        log_execution_stats(scuba, stats, &mut result, log_only);
        result.map_err(|e| e.context(format!("while executing hook {}", hook_name)))
    }

    fn run_hook_on_many_changesets<'this: 'cs, 'ctx: 'this, 'cs, 'provider: 'cs>(
        &'this self,
        ctx: &'ctx CoreContext,
        bookmark: &'cs BookmarkKey,
        changesets: Vec<&'cs BonsaiChangeset>,
        content_provider: &'provider dyn HookStateProvider,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
        hook_name: &'cs str,
        scuba: MononokeScubaSampleBuilder,
        log_only: bool,
    ) -> HooksOutcome<'cs> {
        HooksOutcome::Individual(
            changesets
                .into_iter()
                .map(|cs| {
                    self.run_hook(
                        ctx,
                        bookmark,
                        cs,
                        content_provider,
                        cross_repo_push_source,
                        push_authored_by,
                        hook_name,
                        scuba.clone(),
                        log_only,
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
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'provider: 'change, 'path: 'change>(
        &'this self,
        ctx: &'ctx CoreContext,
        content_provider: &'provider dyn HookStateProvider,
        change: Option<&'change BasicFileChange>,
        path: &'path NonRootMPath,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error>;

    async fn run_hook<'this: 'change, 'ctx: 'this, 'change, 'provider: 'change, 'path: 'change>(
        &'this self,
        ctx: &'ctx CoreContext,
        content_provider: &'provider dyn HookStateProvider,
        change: Option<&'change BasicFileChange>,
        path: &'path NonRootMPath,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
        cs_id: ChangesetId,
        hook_name: &str,
        scuba: MononokeScubaSampleBuilder,
        log_only: bool,
    ) -> Result<HookOutcome, Error> {
        let (stats, mut result) = self
            .run(
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
            .await;
        log_execution_stats(scuba, stats, &mut result, log_only);
        result.map_err(|e| e.context(format!("while executing hook {}", hook_name)))
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
        match self {
            HookOutcome::BookmarkHook(_, HookExecution::Accepted)
            | HookOutcome::ChangesetHook(_, HookExecution::Accepted)
            | HookOutcome::FileHook(_, HookExecution::Accepted) => None,
            HookOutcome::BookmarkHook(
                BookmarkHookExecutionId {
                    cs_id,
                    bookmark_name: _,
                    hook_name,
                },
                HookExecution::Rejected(reason),
            )
            | HookOutcome::ChangesetHook(
                ChangesetHookExecutionId { cs_id, hook_name },
                HookExecution::Rejected(reason),
            )
            | HookOutcome::FileHook(
                FileHookExecutionId {
                    cs_id,
                    hook_name,
                    path: _,
                },
                HookExecution::Rejected(reason),
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
pub enum HookExecution {
    Accepted,
    Rejected(HookRejectionInfo),
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
