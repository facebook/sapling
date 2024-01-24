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
use mononoke_types::BasicFileChange;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::NonRootMPath;

pub use crate::errors::HookFileContentProviderError;
pub use crate::errors::HookManagerError;
pub use crate::manager::HookManager;
pub use crate::provider::memory::InMemoryHookFileContentProvider;
pub use crate::provider::text_only::TextOnlyHookFileContentProvider;
pub use crate::provider::FileChange;
pub use crate::provider::HookFileContentProvider;
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
        content_provider: &'provider dyn HookFileContentProvider,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error>;
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
        content_provider: &'provider dyn HookFileContentProvider,
        change: Option<&'change BasicFileChange>,
        path: &'path NonRootMPath,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error>;
}

/// Outcome of running a hook.
#[derive(Clone, Debug, PartialEq)]
pub enum HookOutcome {
    ChangesetHook(ChangesetHookExecutionId, HookExecution),
    FileHook(FileHookExecutionId, HookExecution),
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

    pub fn get_file_path(&self) -> Option<&NonRootMPath> {
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

    pub fn into_rejection(self) -> Option<HookRejection> {
        match self {
            HookOutcome::ChangesetHook(_, HookExecution::Accepted)
            | HookOutcome::FileHook(_, HookExecution::Accepted) => None,
            HookOutcome::ChangesetHook(
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
