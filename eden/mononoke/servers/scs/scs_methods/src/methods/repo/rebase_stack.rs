/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;

use borrowed::borrowed;
use context::CoreContext;
use futures::try_join;
use futures_watchdog::WatchdogExt;
use maplit::btreemap;
use mononoke_api::ChangesetSpecifier;
use mononoke_api::MononokeError;
use mononoke_api::repo::rebase_stack::RebaseStackMergeResolution;
use mononoke_types::ChangesetId;
use pushrebase::PushrebaseConflict;
use scs_errors::LoggableError;
use scs_errors::ServiceErrorResultExt;
use scs_errors::Status;
use service::RepoRebaseStackExn;
use source_control as thrift;
use source_control_services::errors::source_control_service as service;

use crate::commit_id::map_commit_identities;
use crate::from_request::FromRequest;
use crate::source_control_impl::SourceControlServiceImpl;

pub(crate) enum RebaseStackError {
    Service(scs_errors::ServiceError),
    PushrebaseConflicts(Vec<PushrebaseConflict>),
}

impl From<scs_errors::ServiceError> for RebaseStackError {
    fn from(e: scs_errors::ServiceError) -> Self {
        Self::Service(e)
    }
}

impl From<MononokeError> for RebaseStackError {
    fn from(e: MononokeError) -> Self {
        match e {
            MononokeError::PushrebaseConflicts(conflicts) => Self::PushrebaseConflicts(conflicts),
            e => Self::Service(e.into()),
        }
    }
}

impl From<thrift::RequestError> for RebaseStackError {
    fn from(e: thrift::RequestError) -> Self {
        Self::Service(e.into())
    }
}

fn reason_conflicts(conflicts: &[PushrebaseConflict]) -> String {
    format!("Conflicts while rebasing: {conflicts:?}")
}

impl From<RebaseStackError> for RepoRebaseStackExn {
    fn from(e: RebaseStackError) -> RepoRebaseStackExn {
        match e {
            RebaseStackError::Service(e) => e.into(),
            RebaseStackError::PushrebaseConflicts(conflicts) => {
                RepoRebaseStackExn::pushrebase_conflicts(thrift::PushrebaseConflictsException {
                    reason: reason_conflicts(&conflicts),
                    conflicts: conflicts
                        .into_iter()
                        .map(|c| thrift::PushrebaseConflict {
                            left: c.left.to_string(),
                            right: c.right.to_string(),
                            ..Default::default()
                        })
                        .collect(),
                    ..Default::default()
                })
            }
        }
    }
}

impl LoggableError for RebaseStackError {
    fn status_and_description(&self) -> (Status, String) {
        match self {
            Self::Service(svc) => svc.status_and_description(),
            Self::PushrebaseConflicts(conflicts) => {
                (Status::RequestError, reason_conflicts(conflicts))
            }
        }
    }
}

type IdMap = BTreeMap<ChangesetId, BTreeMap<thrift::CommitIdentityScheme, thrift::CommitId>>;

/// Ids in the requested schemes, or just the bonsai id if the lookup
/// produced nothing for this commit.
fn ids_for(
    map: &IdMap,
    cs_id: ChangesetId,
) -> BTreeMap<thrift::CommitIdentityScheme, thrift::CommitId> {
    match map.get(&cs_id) {
        Some(ids) => ids.clone(),
        None => btreemap! {
            thrift::CommitIdentityScheme::BONSAI =>
                thrift::CommitId::bonsai(cs_id.as_ref().into()),
        },
    }
}

impl SourceControlServiceImpl {
    pub(crate) async fn repo_rebase_stack(
        &self,
        ctx: CoreContext,
        repo: thrift::RepoSpecifier,
        params: thrift::RepoRebaseStackParams,
    ) -> Result<thrift::RepoRebaseStackResponse, RebaseStackError> {
        let repo = self
            .repo_for_service(ctx.clone(), &repo, params.service_identity)
            .watched()
            .await?;
        borrowed!(params.head, params.base, params.onto);
        let head = repo
            .changeset(ChangesetSpecifier::from_request(head)?)
            .watched()
            .await
            .context("failed to resolve head commit")?
            .ok_or_else(|| scs_errors::commit_not_found(head.to_string()))?;
        let base = repo
            .changeset(ChangesetSpecifier::from_request(base)?)
            .watched()
            .await
            .context("failed to resolve base commit")?
            .ok_or_else(|| scs_errors::commit_not_found(base.to_string()))?;
        let onto = repo
            .changeset(ChangesetSpecifier::from_request(onto)?)
            .watched()
            .await
            .context("failed to resolve onto commit")?
            .ok_or_else(|| scs_errors::commit_not_found(onto.to_string()))?;
        let merge_resolution = match params.merge_resolution {
            thrift::RepoRebaseStackMergeResolution::DEFAULT => RebaseStackMergeResolution::Default,
            thrift::RepoRebaseStackMergeResolution::DISABLED => {
                RebaseStackMergeResolution::Disabled
            }
            other => {
                return Err(scs_errors::invalid_request(format!(
                    "unknown merge_resolution {other:?}"
                ))
                .into());
            }
        };

        let outcome = repo
            .rebase_stack(base.id(), head.id(), onto.id(), merge_resolution)
            .watched()
            .await?;

        let old_ids: Vec<ChangesetId> = outcome.commits.iter().map(|c| c.old).collect();
        let new_ids: Vec<ChangesetId> = outcome
            .commits
            .iter()
            .filter_map(|c| c.new)
            .chain(std::iter::once(outcome.head))
            .collect();
        let old_schemes = params
            .old_identity_schemes
            .as_ref()
            .unwrap_or(&params.identity_schemes);
        let (old_map, new_map) = try_join!(
            map_commit_identities(&repo, old_ids, old_schemes),
            map_commit_identities(&repo, new_ids, &params.identity_schemes),
        )?;

        let rebased_commits = outcome
            .commits
            .into_iter()
            .map(|c| thrift::RepoRebaseStackRebasedCommit {
                old_ids: ids_for(&old_map, c.old),
                new_ids: c.new.map(|n| ids_for(&new_map, n)).unwrap_or_default(),
                merged_paths: c.merged_paths.iter().map(ToString::to_string).collect(),
                dropped: c.new.is_none(),
                ..Default::default()
            })
            .collect();
        Ok(thrift::RepoRebaseStackResponse {
            head: ids_for(&new_map, outcome.head),
            rebased_commits,
            overlapping_path_count: outcome.overlapping_path_count as i64,
            merged_path_count: outcome.merged_path_count as i64,
            ..Default::default()
        })
    }
}
