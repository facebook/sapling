/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::types::{Source, Target};
use anyhow::Error;
use context::CoreContext;
use futures::compat::Future01CompatExt;
use metaconfig_types::CommitSyncConfigVersion;
use mononoke_types::{ChangesetId, RepositoryId};
use synced_commit_mapping::{SyncedCommitMapping, WorkingCopyEquivalence};

/// The state of a source repo commit in a target repo
#[derive(Debug, PartialEq)]
pub enum CommitSyncOutcome {
    /// Not suitable for syncing to this repo
    NotSyncCandidate,
    /// This commit is a 1:1 semantic mapping, but sync process rewrote it to a new ID.
    RewrittenAs(ChangesetId, Option<CommitSyncConfigVersion>),
    /// This commit is exactly identical in the target repo
    Preserved,
    /// This commit is removed by the sync process, and the commit with the given ID has same content
    EquivalentWorkingCopyAncestor(ChangesetId),
}

pub async fn get_commit_sync_outcome<'a, M: SyncedCommitMapping>(
    ctx: &'a CoreContext,
    source_repo_id: Source<RepositoryId>,
    target_repo_id: Target<RepositoryId>,
    source_cs_id: Source<ChangesetId>,
    mapping: &'a M,
) -> Result<Option<CommitSyncOutcome>, Error> {
    let remapped = mapping
        .get_one(
            ctx.clone(),
            source_repo_id.0,
            source_cs_id.0,
            target_repo_id.0,
        )
        .compat()
        .await?;

    if let Some((cs_id, maybe_version)) = remapped {
        // If we have a mapping for this commit, then it is already synced
        if cs_id == source_cs_id.0 {
            return Ok(Some(CommitSyncOutcome::Preserved));
        } else {
            return Ok(Some(CommitSyncOutcome::RewrittenAs(cs_id, maybe_version)));
        }
    }

    let maybe_wc_equivalence = mapping
        .clone()
        .get_equivalent_working_copy(
            ctx.clone(),
            source_repo_id.0,
            source_cs_id.0,
            target_repo_id.0,
        )
        .compat()
        .await?;

    match maybe_wc_equivalence {
        None => Ok(None),
        Some(WorkingCopyEquivalence::NoWorkingCopy) => {
            Ok(Some(CommitSyncOutcome::NotSyncCandidate))
        }
        Some(WorkingCopyEquivalence::WorkingCopy(cs_id)) => {
            if source_cs_id.0 == cs_id {
                Ok(Some(CommitSyncOutcome::Preserved))
            } else {
                Ok(Some(CommitSyncOutcome::EquivalentWorkingCopyAncestor(
                    cs_id,
                )))
            }
        }
    }
}
