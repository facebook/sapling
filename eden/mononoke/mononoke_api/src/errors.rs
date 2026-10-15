/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::backtrace::Backtrace;
use std::convert::Infallible;
use std::error::Error as StdError;
use std::fmt;
use std::sync::Arc;

use anyhow::Error;
use blame::BlameError;
use blobrepo_hg::AclFileChangeRejectedInChangeset;
use blobstore::LoadableError;
use bookmarks::BookmarkKey;
use bookmarks_movement::BookmarkMovementError;
use bookmarks_movement::HookRejection;
use bookmarks_movement::describe_hook_rejections;
use commit_cloud_types::CommitCloudError;
use derived_data::DerivationError;
use derived_data::SharedDerivationError;
use itertools::Itertools;
use megarepo_error::MegarepoError;
use mononoke_types::ChangesetId;
use mononoke_types::path::MPath;
use pushrebase::PushrebaseError;
use redactedblobstore::has_redaction_root_cause;
use repo_authorization::AuthorizationError;
use restricted_paths::AclFileChangeRejection;
use restricted_paths::RestrictedPathsAuthorizationError;
use restricted_paths::RestrictedPathsError;
use thiserror::Error;
use tokio::task::JoinError;

#[derive(Clone, Debug)]
pub struct InternalError(Arc<Error>);

// The cargo build of anyhow disables its backtrace features when using RUSTC_BOOTSTRAP=1
#[cfg(not(fbcode_build))]
static DISABLED: Backtrace = Backtrace::disabled();

impl InternalError {
    #[cfg(fbcode_build)]
    pub fn backtrace(&self) -> &Backtrace {
        self.0.backtrace()
    }

    #[cfg(not(fbcode_build))]
    pub fn backtrace(&self) -> &Backtrace {
        &DISABLED
    }
}

impl fmt::Display for InternalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<Error> for InternalError {
    fn from(error: Error) -> Self {
        Self(Arc::new(error))
    }
}

impl StdError for InternalError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(&**self.0)
    }

    #[cfg(fbcode_build)]
    fn provide<'a>(&'a self, request: &mut ::std::error::Request<'a>) {
        request.provide_ref::<Backtrace>(self.backtrace());
    }
}

#[derive(Clone, Debug, Error)]
pub enum MononokeError {
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("Large repo '{0}' not found")]
    LargeRepoNotFound(String),
    #[error("unresolved path conflicts in merge:\n {}", .conflict_paths.iter().join("\n"))]
    MergeConflicts { conflict_paths: Vec<MPath> },
    #[error("Conflicts while pushrebasing: {0:?}")]
    PushrebaseConflicts(Vec<pushrebase::PushrebaseConflict>),
    #[error("Non fast-forward bookmark move of '{bookmark}' from {from} to {to}")]
    NonFastForwardMove {
        bookmark: BookmarkKey,
        from: ChangesetId,
        to: ChangesetId,
    },
    #[error(
        "permission denied: access to repo {reponame} on behalf of {service_identity} not permitted for {identities}"
    )]
    ServicePermissionDenied {
        identities: String,
        reponame: String,
        service_identity: String,
    },
    #[error("hooks failed:\n{}", describe_hook_rejections(.0.as_slice()))]
    HookFailure(Vec<HookRejection>),
    #[error("not available: {0}")]
    NotAvailable(String),
    #[error("manifest not derived for {0}")]
    ManifestNotDerived(ChangesetId),
    #[error("permission denied: {0}")]
    AuthorizationError(String),
    #[error("permission denied: {0}")]
    RestrictedPathsAuthorizationError(#[source] RestrictedPathsAuthorizationError),
    #[error("internal error: {0}")]
    InternalError(#[source] InternalError),
    #[error("The blob {key} was redacted due to {reason}")]
    RedactionError { key: String, reason: String },
    #[error("Bookmark move already processed")]
    BookmarkMoveAlreadyProcessed,
}

impl MononokeError {
    /// True only for restricted-path denials (spawn_enforce_restricted_path_access).
    /// Repo-auth denials (`AuthorizationError`) and everything else fail the whole
    /// request; only this variant is safe to convert into a per-item skip.
    pub fn is_restricted_path_denial(&self) -> bool {
        matches!(self, MononokeError::RestrictedPathsAuthorizationError(_))
    }

    /// The restricted-path denial, if this error is one.
    pub fn restricted_paths_denial(&self) -> Option<&RestrictedPathsAuthorizationError> {
        match self {
            Self::RestrictedPathsAuthorizationError(e) => Some(e),
            _ => None,
        }
    }
}

impl From<Error> for MononokeError {
    /// `.slacl` rejections from Hg changeset creation are reported as the
    /// rejection itself, without the creation context around it.
    fn from(e: Error) -> Self {
        if let Some(rejected) = e
            .root_cause()
            .downcast_ref::<AclFileChangeRejectedInChangeset>()
        {
            return acl_file_change_rejection_error(&rejected.rejection, rejected.to_string());
        }
        match has_redaction_root_cause(&e) {
            Some((key, reason)) => MononokeError::RedactionError {
                key: key.to_string(),
                reason: reason.to_string(),
            },
            _ => MononokeError::InternalError(InternalError(Arc::new(e))),
        }
    }
}

impl From<Infallible> for MononokeError {
    fn from(_i: Infallible) -> Self {
        unreachable!()
    }
}

impl From<LoadableError> for MononokeError {
    fn from(e: LoadableError) -> Self {
        MononokeError::InternalError(InternalError(Arc::new(e.into())))
    }
}

impl From<DerivationError> for MononokeError {
    fn from(e: DerivationError) -> Self {
        match e {
            e @ DerivationError::Disabled(..) => MononokeError::NotAvailable(e.to_string()),
            e => MononokeError::from(anyhow::Error::from(e)),
        }
    }
}

impl From<SharedDerivationError> for MononokeError {
    fn from(e: SharedDerivationError) -> Self {
        match e.inner() {
            inner @ DerivationError::Disabled(..) => MononokeError::NotAvailable(inner.to_string()),
            _ => MononokeError::from(anyhow::Error::from(e)),
        }
    }
}

impl From<BookmarkMovementError> for MononokeError {
    fn from(e: BookmarkMovementError) -> Self {
        match e {
            BookmarkMovementError::AuthorizationError(e) => {
                MononokeError::AuthorizationError(e.to_string())
            }
            BookmarkMovementError::HookFailure(rejections) => {
                MononokeError::HookFailure(rejections)
            }
            BookmarkMovementError::PushrebaseError(PushrebaseError::Conflicts(conflicts)) => {
                MononokeError::PushrebaseConflicts(conflicts)
            }
            BookmarkMovementError::NonFastForwardMove { bookmark, from, to } => {
                MononokeError::NonFastForwardMove { bookmark, from, to }
            }
            BookmarkMovementError::Error(e) => MononokeError::InternalError(InternalError::from(e)),
            BookmarkMovementError::AlreadyProcessed => MononokeError::BookmarkMoveAlreadyProcessed,
            _ => MononokeError::InvalidRequest(e.to_string()),
        }
    }
}

impl From<AuthorizationError> for MononokeError {
    fn from(e: AuthorizationError) -> Self {
        match e {
            AuthorizationError::PermissionDenied(e) => {
                MononokeError::AuthorizationError(e.to_string())
            }
            AuthorizationError::Error(e) => MononokeError::InternalError(InternalError::from(e)),
        }
    }
}

impl From<BlameError> for MononokeError {
    fn from(e: BlameError) -> Self {
        use BlameError::*;
        match e {
            NoSuchPath(_) | IsDirectory(_) | Rejected(_) => {
                MononokeError::InvalidRequest(e.to_string())
            }
            DerivationError(e) => MononokeError::from(e),
            _ => MononokeError::from(anyhow::Error::from(e)),
        }
    }
}

impl From<MononokeError> for edenapi_types::ServerError {
    fn from(e: MononokeError) -> Self {
        edenapi_types::ServerError::from(&e)
    }
}

impl From<&MononokeError> for edenapi_types::ServerError {
    fn from(e: &MononokeError) -> Self {
        let message = format!("{e:?}");
        Self::new(message, 1)
    }
}

impl From<MononokeError> for MegarepoError {
    fn from(e: MononokeError) -> Self {
        MegarepoError::internal(e)
    }
}

impl From<CommitCloudError> for MononokeError {
    fn from(e: CommitCloudError) -> Self {
        match e {
            CommitCloudError::InternalError(e) => {
                MononokeError::InternalError(InternalError(Arc::new(e.into())))
            }
            CommitCloudError::UserError(e) => MononokeError::InvalidRequest(e.to_string()),
        }
    }
}

impl From<JoinError> for MononokeError {
    fn from(e: JoinError) -> Self {
        MononokeError::from(anyhow::Error::from(e))
    }
}

impl From<RestrictedPathsError> for MononokeError {
    fn from(e: RestrictedPathsError) -> Self {
        match e {
            RestrictedPathsError::AuthorizationError(err) => {
                MononokeError::RestrictedPathsAuthorizationError(err)
            }
            RestrictedPathsError::AclFileChangeRejected(rejection) => {
                MononokeError::from(rejection)
            }
            RestrictedPathsError::InternalError(err) => MononokeError::InternalError(err.into()),
        }
    }
}

impl From<AclFileChangeRejection> for MononokeError {
    fn from(rejection: AclFileChangeRejection) -> Self {
        acl_file_change_rejection_error(&rejection, rejection.to_string())
    }
}

/// Map an ACL file change rejection to the `MononokeError` variant for its
/// kind, with `message` as the error text. `message` is separate so that a
/// wrapped rejection, e.g. [`AclFileChangeRejectedInChangeset`], keeps the
/// wrapper's full text.
fn acl_file_change_rejection_error(
    rejection: &AclFileChangeRejection,
    message: String,
) -> MononokeError {
    match rejection {
        AclFileChangeRejection::Unauthorized { .. } => MononokeError::AuthorizationError(message),
        AclFileChangeRejection::ModifiedEarlierInStack { .. } => {
            MononokeError::InvalidRequest(message)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use anyhow::Result;
    use anyhow::format_err;
    use mercurial_types::HgChangesetId;
    use mononoke_macros::mononoke;
    use mononoke_types::NonRootMPath;
    use permission_checker::MononokeIdentity;
    use restricted_paths::DenyingRestriction;
    use restricted_paths::RestrictedPathAccess;
    use shared_error::anyhow::SharedError;

    use super::*;

    /// What it tests: `restricted_paths_denial` exposes the denial error.
    /// Expected: `Some` carrying the error for restricted-path denials,
    /// `None` for repo-auth denials and everything else.
    #[mononoke::test]
    fn test_restricted_paths_denial_returns_error_for_denials_only() -> Result<()> {
        let acl = MononokeIdentity::from_str("REPO_REGION:acl")?;
        let denial = RestrictedPathsAuthorizationError::new(
            RestrictedPathAccess::Path(MPath::new("restricted/file")?),
            DenyingRestriction {
                restriction_root: Some(NonRootMPath::new("restricted")?),
                repo_region_acl: acl.clone(),
                permission_request_group: acl,
            },
            None,
        );
        let denial_error = MononokeError::RestrictedPathsAuthorizationError(denial);
        assert!(denial_error.is_restricted_path_denial());
        let back = denial_error
            .restricted_paths_denial()
            .expect("a denial should expose its error");
        assert_eq!(
            back.restriction_root(),
            Some(&NonRootMPath::new("restricted")?)
        );

        let repo_denial = MononokeError::AuthorizationError("repo denied".to_string());
        assert!(!repo_denial.is_restricted_path_denial());
        assert!(repo_denial.restricted_paths_denial().is_none());
        Ok(())
    }

    /// What it tests: an unauthorized `.slacl` change rejected while creating
    /// an Hg changeset, reported through a child commit's shared error.
    /// Expected: a permission error whose message is the rejection reason
    /// naming the rejected changeset.
    #[mononoke::test]
    fn test_from_error_reports_unauthorized_acl_file_change() -> Result<()> {
        let rejection = AclFileChangeRejection::Unauthorized {
            dir: MPath::new("secret")?,
            repo_region_acl: "REPO_REGION:acl".to_string(),
            restriction_root: NonRootMPath::new("secret")?,
        };
        let reason = "Cannot modify ACL file at secret: directory is restricted by REPO_REGION:acl (restriction root: secret). Caller does not have maintainer access. (changeset 1111111111111111111111111111111111111111)";

        let error = MononokeError::from(child_changeset_error(rejected_in_changeset(rejection)?));

        assert!(
            matches!(&error, MononokeError::AuthorizationError(r) if r == reason),
            "expected AuthorizationError({reason:?}), got {error:#?}"
        );
        assert_eq!(error.to_string(), format!("permission denied: {reason}"));
        Ok(())
    }

    /// What it tests: a `.slacl` change rejected by the stack rule while
    /// creating an Hg changeset.
    /// Expected: an invalid request whose message is the rejection reason
    /// naming the rejected changeset.
    #[mononoke::test]
    fn test_from_error_reports_acl_file_stack_rejection() -> Result<()> {
        let rejection = AclFileChangeRejection::ModifiedEarlierInStack {
            dir: MPath::new("secret")?,
        };
        let reason = "Cannot modify ACL file at secret: .slacl at this path was already modified by an earlier changeset in this stack. Split into separate submissions. (changeset 1111111111111111111111111111111111111111)";

        let error = MononokeError::from(child_changeset_error(rejected_in_changeset(rejection)?));

        assert!(
            matches!(&error, MononokeError::InvalidRequest(r) if r == reason),
            "expected InvalidRequest({reason:?}), got {error:#?}"
        );
        Ok(())
    }

    /// What it tests: changeset creation failures that are not `.slacl`
    /// rejections, including a restricted paths error that is not a
    /// rejection.
    /// Expected: they stay internal errors.
    #[mononoke::test]
    fn test_from_error_keeps_other_changeset_creation_errors_internal() {
        let failures = [
            format_err!("Inconsistent node hash for changeset"),
            RestrictedPathsError::InternalError(format_err!("ACL lookup failed")).into(),
        ];

        for failure in failures {
            let error = MononokeError::from(child_changeset_error(failure));

            assert!(
                matches!(error, MononokeError::InternalError(_)),
                "expected InternalError, got {error:#?}"
            );
        }
    }

    fn rejected_in_changeset(rejection: AclFileChangeRejection) -> Result<Error> {
        Ok(AclFileChangeRejectedInChangeset {
            hg_cs_id: HgChangesetId::from_str("1111111111111111111111111111111111111111")?,
            rejection,
        }
        .into())
    }

    /// Wrap `failure` the way `CreateChangeset` reports it for the child of
    /// the commit that failed: the parent's error gets creation context and is
    /// shared through its `ChangesetHandle`, then the child adds its own
    /// context and is shared again.
    fn child_changeset_error(failure: Error) -> Error {
        let parent = SharedError::from(Arc::new(
            failure
                .context("While validating ACL file changes")
                .context("While creating Changeset parent"),
        ));
        let child = SharedError::from(Arc::new(
            Error::from(parent).context("While creating Changeset child"),
        ));
        Error::from(child)
    }
}
