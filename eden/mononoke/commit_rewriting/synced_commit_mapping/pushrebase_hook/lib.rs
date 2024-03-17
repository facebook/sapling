/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use ::pushrebase_hook::PushrebaseCommitHook;
use ::pushrebase_hook::PushrebaseHook;
use ::pushrebase_hook::PushrebaseTransactionHook;
use ::pushrebase_hook::RebasedChangesets;
use anyhow::anyhow;
use anyhow::format_err;
use anyhow::Context;
use anyhow::Error;
use async_trait::async_trait;
use bookmarks::BookmarkTransactionError;
use context::CoreContext;
use metaconfig_types::CommitSyncConfigVersion;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use sql::Transaction;
use synced_commit_mapping::add_many_in_txn;
use synced_commit_mapping::add_many_large_repo_commit_versions_in_txn;
use synced_commit_mapping::SyncedCommitMapping;
use synced_commit_mapping::SyncedCommitMappingEntry;
use synced_commit_mapping::SyncedCommitSourceRepo;

const CHANGE_XREPO_MAPPING_EXTRA: &str = "change-xrepo-mapping-to-version";
/// TODO(mitrandir) This function is copied from cross_repo_sync crate. It needs to be refactored into separate library.
fn get_mapping_change_version_from_hg_extra<'a>(
    mut hg_extra: impl Iterator<Item = (&'a str, &'a [u8])>,
) -> Result<Option<CommitSyncConfigVersion>, Error> {
    if justknobs::eval("scm/mononoke:ignore_change_xrepo_mapping_extra", None, None)
        .unwrap_or(false)
    {
        return Ok(None);
    }
    let maybe_mapping = hg_extra.find(|(name, _)| name == &CHANGE_XREPO_MAPPING_EXTRA);
    if let Some((_, version)) = maybe_mapping {
        let version = String::from_utf8(version.to_vec())
            .with_context(|| "non-utf8 version change".to_string())?;

        let version = CommitSyncConfigVersion(version);
        Ok(Some(version))
    } else {
        Ok(None)
    }
}

fn get_mapping_change_version_from_bonsai_changeset_mut(
    bcs: &BonsaiChangesetMut,
) -> Result<Option<CommitSyncConfigVersion>, Error> {
    get_mapping_change_version_from_hg_extra(
        bcs.hg_extra.iter().map(|(k, v)| (k.as_str(), v.as_slice())),
    )
}

pub fn create_synced_commit_mapping_entry(
    forward_synced_commit_info: ForwardSyncedCommitInfo,
    large_bcs_id: ChangesetId,
) -> SyncedCommitMappingEntry {
    let ForwardSyncedCommitInfo {
        small_bcs_id,
        large_repo_id,
        small_repo_id,
        version_name,
    } = forward_synced_commit_info;
    SyncedCommitMappingEntry {
        large_repo_id,
        large_bcs_id,
        small_repo_id,
        small_bcs_id,
        version_name: Some(version_name),
        source_repo: Some(SyncedCommitSourceRepo::Small),
    }
}
/// Structure present if given pushrebase is a forward (small-to-large) sync and mapping should be
/// updated accordingly.
#[derive(Clone)]
pub struct ForwardSyncedCommitInfo {
    pub small_bcs_id: ChangesetId,
    pub large_repo_id: RepositoryId,
    pub small_repo_id: RepositoryId,
    pub version_name: CommitSyncConfigVersion,
}

/// CrossRepoSyncPushrebase hook is reponsible for updating the mapping versions of pushrebased commits for
/// repos that have cross-repo-sync enabled. In particular that means:
///  1. Updating the large_repo_commit_version for all commits that were pushrebased
///    (the mappings will be assigned by backsyncer once it gets to those)
///  2. Updating small-to-large mapping for commtis that were forward-synced from small repo via pushrebase:
///    as those commits are created during pushrebase transaction it's good to update the mapping atomically
///  3. Verifying that the forward syncer doesn't accidentally change the mapping version.
///
/// If the large_repo_commit version is not assigned for parent commit the hook does not fail but restricts its
/// operation to only updating the mapping for the forward synced commits.
#[derive(Clone)]
pub struct CrossRepoSyncPushrebaseHook<M> {
    synced_commit_mapping: M,
    large_repo_id: RepositoryId,
    forward_synced_commit_info: Option<ForwardSyncedCommitInfo>,
}

impl<M: SyncedCommitMapping + Clone + 'static> CrossRepoSyncPushrebaseHook<M> {
    pub fn new(
        synced_commit_mapping: M,
        large_repo_id: RepositoryId,
        forward_synced_commit_info: Option<ForwardSyncedCommitInfo>,
    ) -> Box<dyn PushrebaseHook> {
        Box::new(Self {
            synced_commit_mapping,
            large_repo_id,
            forward_synced_commit_info,
        })
    }
}

#[async_trait]
impl<M: SyncedCommitMapping + Clone + 'static> PushrebaseHook for CrossRepoSyncPushrebaseHook<M> {
    async fn in_critical_section(
        &self,
        ctx: &CoreContext,
        old_bookmark_value: Option<ChangesetId>,
    ) -> Result<Box<dyn PushrebaseCommitHook>, Error> {
        // TODO(mitrandir): cleanup this justknob once rolled out
        let old_version = if justknobs::eval(
            "scm/mononoke:xrepo_assign_large_repo_version_on_pushrebase",
            None,
            None,
        )
        .unwrap_or(true)
        {
            if let Some(old_bookmark_value) = old_bookmark_value {
                self.synced_commit_mapping
                    .get_large_repo_commit_version(ctx, self.large_repo_id, old_bookmark_value)
                    .await?
            } else {
                return Err(format_err!(
                    "all pushrebase bookmarks need to be initialized when cross-repo-sync is enabled"
                ));
            }
        } else {
            None
        };

        let hook = Box::new(CrossRepoSyncPushrebaseCommitHook {
            forward_synced_commit_info: self.forward_synced_commit_info.clone(),
            version: old_version,
            large_repo_version_assignments: HashMap::new(),
            large_repo_id: self.large_repo_id,
        }) as Box<dyn PushrebaseCommitHook>;
        Ok(hook)
    }
}

#[derive(Clone)]
pub struct CrossRepoSyncPushrebaseCommitHook {
    version: Option<CommitSyncConfigVersion>,
    forward_synced_commit_info: Option<ForwardSyncedCommitInfo>,
    large_repo_version_assignments: HashMap<ChangesetId, CommitSyncConfigVersion>,
    large_repo_id: RepositoryId,
}

#[async_trait]
impl PushrebaseCommitHook for CrossRepoSyncPushrebaseCommitHook {
    fn post_rebase_changeset(
        &mut self,
        bcs_old: ChangesetId,
        bcs_new: &mut BonsaiChangesetMut,
    ) -> Result<(), Error> {
        if let Some(changed_version) =
            get_mapping_change_version_from_bonsai_changeset_mut(bcs_new)?
        {
            self.version = Some(changed_version);
        }
        if let Some(forward_synced_commit_info) = &self.forward_synced_commit_info {
            // Let's validate that we used the right version for forward sync. Race condition happen
            // and in such cases it's better to fail than to revert the version back.
            if let Some(version) = &self.version {
                if version != &forward_synced_commit_info.version_name {
                    return Err(format_err!(
                        "version mismatch for forward synced commit: expected {}, got {}",
                        forward_synced_commit_info.version_name,
                        version
                    ));
                }
            }
        } else if let Some(version) = &self.version {
            self.large_repo_version_assignments
                .insert(bcs_old, version.clone());
        }
        Ok(())
    }

    async fn into_transaction_hook(
        self: Box<Self>,
        _ctx: &CoreContext,
        rebased: &RebasedChangesets,
    ) -> Result<Box<dyn PushrebaseTransactionHook>, Error> {
        if let Some(forward_synced_commit_info) = self.forward_synced_commit_info {
            if rebased.len() > 1 {
                return Err(format_err!("expected exactly one commit to be rebased"));
            }

            match rebased.iter().next() {
                Some((_, (new_cs_id, _))) => {
                    let entry =
                        create_synced_commit_mapping_entry(forward_synced_commit_info, *new_cs_id);
                    Ok(Box::new(CrossRepoSyncTransactionHook {
                        forward_synced_entry: Some(entry),
                        large_repo_version_assignments: vec![],
                    }) as Box<dyn PushrebaseTransactionHook>)
                }
                None => {
                    return Err(format_err!("expected exactly one commit to be rebased"));
                }
            }
        } else {
            let large_repo_version_assignments = self
                 .large_repo_version_assignments
                 .into_iter()
                 .map(|(cs_id, version)| {
                     let replacement_bcs_id = rebased
                         .get(&cs_id)
                         .ok_or_else(|| {
                             let e = format!(
                                 "Commit was assigned a version, but is not found in rebased set: {}",
                                 cs_id
                             );
                             Error::msg(e)
                         })?
                         .0;

                     let var_name = (self.large_repo_id, replacement_bcs_id, version);
                     Ok(var_name)
                 })
                 .collect::<Result<Vec<_>, Error>>()?;

            Ok(Box::new(CrossRepoSyncTransactionHook {
                forward_synced_entry: None,
                large_repo_version_assignments,
            }) as Box<dyn PushrebaseTransactionHook>)
        }
    }
}

#[derive(Clone)]
pub struct CrossRepoSyncTransactionHook {
    forward_synced_entry: Option<SyncedCommitMappingEntry>,
    large_repo_version_assignments: Vec<(RepositoryId, ChangesetId, CommitSyncConfigVersion)>,
}

#[async_trait]
impl PushrebaseTransactionHook for CrossRepoSyncTransactionHook {
    async fn populate_transaction(
        &self,
        _ctx: &CoreContext,
        txn: Transaction,
    ) -> Result<Transaction, BookmarkTransactionError> {
        let txn = if let Some(entry) = &self.forward_synced_entry {
            let xrepo_sync_disable_all_syncs =
                justknobs::eval("scm/mononoke:xrepo_sync_disable_all_syncs", None, None)
                    .unwrap_or_default();
            if xrepo_sync_disable_all_syncs {
                return Err(anyhow!(
                    "X-repo sync is temporarily disabled, contact source control oncall"
                )
                .into());
            }
            add_many_in_txn(txn, vec![entry.clone()]).await?.0
        } else {
            txn
        };
        let txn = if !self.large_repo_version_assignments.is_empty() {
            add_many_large_repo_commit_versions_in_txn(txn, &self.large_repo_version_assignments)
                .await?
                .0
        } else {
            txn
        };
        Ok(txn)
    }
}
