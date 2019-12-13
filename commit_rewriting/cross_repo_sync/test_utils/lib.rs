/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use futures::Future;

use anyhow::{format_err, Error};
use bookmarks::{BookmarkName, BookmarkUpdateReason};
use context::CoreContext;
use mononoke_types::ChangesetId;
use std::collections::HashMap;
use synced_commit_mapping::{SyncedCommitMapping, SyncedCommitMappingEntry};

use cross_repo_sync::{rewrite_commit_compat, upload_commits_compat, CommitSyncer};

// Helper function that takes a root commit from source repo and rebases it on master bookmark
// in target repo
pub fn rebase_root_on_master<M>(
    ctx: CoreContext,
    commit_syncer: &CommitSyncer<M>,
    source_bcs_id: ChangesetId,
) -> Result<ChangesetId, Error>
where
    M: SyncedCommitMapping + Clone + 'static,
{
    let bookmark_name = BookmarkName::new("master").unwrap();
    let source_bcs = commit_syncer
        .get_source_repo()
        .get_bonsai_changeset(ctx.clone(), source_bcs_id)
        .wait()
        .unwrap();
    if !source_bcs.parents().collect::<Vec<_>>().is_empty() {
        return Err(format_err!("not a root commit"));
    }

    let maybe_bookmark_val = commit_syncer
        .get_target_repo()
        .get_bonsai_bookmark(ctx.clone(), &bookmark_name)
        .wait()?;

    let source_repo = commit_syncer.get_source_repo();
    let target_repo = commit_syncer.get_target_repo();

    let bookmark_val = maybe_bookmark_val.ok_or(format_err!("master not found"))?;
    let source_bcs_mut = source_bcs.into_mut();
    let maybe_rewritten = rewrite_commit_compat(
        ctx.clone(),
        source_bcs_mut,
        HashMap::new(),
        commit_syncer.get_mover().clone(),
        source_repo.clone(),
    )
    .wait()?;
    let mut target_bcs_mut = maybe_rewritten.unwrap();
    target_bcs_mut.parents = vec![bookmark_val];

    let target_bcs = target_bcs_mut.freeze()?;
    upload_commits_compat(
        ctx.clone(),
        vec![target_bcs.clone()],
        commit_syncer.get_source_repo().clone(),
        commit_syncer.get_target_repo().clone(),
    )
    .wait()?;

    let mut txn = target_repo.update_bookmark_transaction(ctx.clone());
    txn.force_set(
        &bookmark_name,
        target_bcs.get_changeset_id(),
        BookmarkUpdateReason::TestMove {
            bundle_replay_data: None,
        },
    )
    .unwrap();
    txn.commit().wait().unwrap();

    let entry = SyncedCommitMappingEntry::new(
        target_repo.get_repoid(),
        target_bcs.get_changeset_id(),
        source_repo.get_repoid(),
        source_bcs_id,
    );
    commit_syncer.get_mapping().add(ctx.clone(), entry).wait()?;

    Ok(target_bcs.get_changeset_id())
}
