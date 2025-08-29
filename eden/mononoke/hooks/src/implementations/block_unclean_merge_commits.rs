/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use async_trait::async_trait;
use bookmarks::BookmarkKey;
use commit_graph::CommitGraphArc;
use context::CoreContext;
use fsnodes::RootFsnodeId;
use futures::stream;
use futures::stream::TryStreamExt;
use itertools::Itertools;
use manifest::ManifestOps;
use metaconfig_types::HookConfig;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::FileChange;
use mononoke_types::NonRootMPath;
use regex::Regex;
use repo_blobstore::RepoBlobstoreArc;
use repo_derived_data::RepoDerivedDataRef;
use serde::Deserialize;

use crate::ChangesetHook;
use crate::CrossRepoPushSource;
use crate::HookExecution;
use crate::HookRejectionInfo;
use crate::HookRepo;
use crate::PushAuthoredBy;

#[derive(Clone, Debug, Deserialize)]
pub struct BlockUncleanMergeCommitsConfig {
    #[serde(default, with = "serde_regex")]
    only_check_branches_matching_regex: Option<Regex>,
}

#[derive(Clone, Debug)]
pub struct BlockUncleanMergeCommitsHook {
    config: BlockUncleanMergeCommitsConfig,
}

impl BlockUncleanMergeCommitsHook {
    pub fn new(config: &HookConfig) -> Result<Self> {
        Self::with_config(config.parse_options()?)
    }

    pub fn with_config(config: BlockUncleanMergeCommitsConfig) -> Result<Self> {
        Ok(Self { config })
    }
}

#[async_trait]
impl ChangesetHook for BlockUncleanMergeCommitsHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'repo: 'cs>(
        &'this self,
        ctx: &'ctx CoreContext,
        hook_repo: &'repo HookRepo,
        bookmark: &BookmarkKey,
        changeset: &'cs BonsaiChangeset,
        _cross_repo_push_source: CrossRepoPushSource,
        _push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        if bookmark.is_tag() {
            return Ok(HookExecution::Accepted);
        }

        if !changeset.is_merge() {
            return Ok(HookExecution::Accepted);
        }

        let fail_msg = if let Some(regex) = &self.config.only_check_branches_matching_regex {
            if !regex.is_match(bookmark.as_str()) {
                return Ok(HookExecution::Accepted);
            } else {
                format!(
                    "The bookmark matching regex {} can't have merge commits with conflicts, even if they have been resolved",
                    regex
                )
            }
        } else {
            "The repo can't have merge commits with conflicts, even if they have been resolved"
                .to_string()
        };

        let parent_root_fsnodes: HashMap<ChangesetId, RootFsnodeId> =
            stream::iter(changeset.parents().map(|p| {
                Ok(async move {
                    let root_fsnode_id = hook_repo
                        .repo_derived_data()
                        .derive::<RootFsnodeId>(ctx, p)
                        .await
                        .with_context(|| "Can't lookup RootFsnodeId for ChangesetId")?;
                    Ok::<_, anyhow::Error>((p, root_fsnode_id))
                })
            }))
            .try_buffered(5)
            .try_collect()
            .await?;

        for (path, file_change) in changeset.file_changes() {
            if !is_file_change_clean(
                ctx,
                path,
                file_change,
                changeset,
                hook_repo,
                &parent_root_fsnodes,
            )
            .await?
            {
                return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                    "Merge commits with conflicts, even if resolved are not allowed. If you think this is a false-positive, please add '@allow-unclean-merges' as a standalone line to the commit message",
                    fail_msg,
                )));
            }
        }

        Ok(HookExecution::Accepted)
    }
}

async fn is_file_change_clean(
    ctx: &CoreContext,
    path: &NonRootMPath,
    file_change: &FileChange,
    changeset: &BonsaiChangeset,
    hook_repo: &HookRepo,
    parent_root_fsnodes: &HashMap<ChangesetId, RootFsnodeId>,
) -> Result<bool> {
    match file_change {
        FileChange::Change(_) | FileChange::UntrackedChange(_) => {
            is_file_change_change_clean(
                ctx,
                path,
                file_change,
                changeset,
                hook_repo,
                parent_root_fsnodes,
            )
            .await
        }
        FileChange::Deletion | FileChange::UntrackedDeletion => {
            is_file_change_deletion_clean(ctx, path, changeset, hook_repo, parent_root_fsnodes)
                .await
        }
    }
}

async fn is_file_change_change_clean(
    ctx: &CoreContext,
    path: &NonRootMPath,
    file_change: &FileChange,
    changeset: &BonsaiChangeset,
    hook_repo: &HookRepo,
    parent_root_fsnodes: &HashMap<ChangesetId, RootFsnodeId>,
) -> Result<bool> {
    let mut parents_with_different_content = 0;
    for parent in changeset.parents() {
        let parent_root_fsnode_id = parent_root_fsnodes
            .get(&parent)
            .ok_or_else(|| anyhow!("Can't find previously stored RootFsnodeId"))?;
        if !change_the_same_in_parent(ctx, path, file_change, parent_root_fsnode_id, hook_repo)
            .await?
        {
            parents_with_different_content += 1;
        }

        if parents_with_different_content > 1 {
            return Ok(false);
        }
    }

    // All parents have the same content or just one.
    Ok(true)
}

async fn is_file_change_deletion_clean(
    ctx: &CoreContext,
    path: &NonRootMPath,
    changeset: &BonsaiChangeset,
    hook_repo: &HookRepo,
    parent_root_fsnodes: &HashMap<ChangesetId, RootFsnodeId>,
) -> Result<bool> {
    // Here, git straight up refuses to merge many branches if there are conflicts.
    if changeset.parents().count() > 2 {
        return Ok(false);
    }

    let (parent1, parent2) = changeset
        .parents()
        .collect_tuple()
        .ok_or_else(|| anyhow!("More than 2 expected parents"))?;

    let parent1_entry = parent_root_fsnodes
        .get(&parent1)
        .ok_or_else(|| anyhow!("Can't find previously stored RootFsnodeId"))?
        .fsnode_id()
        .find_entry(
            ctx.clone(),
            hook_repo.repo_blobstore_arc(),
            path.clone().into(),
        )
        .await?;

    let parent2_entry = parent_root_fsnodes
        .get(&parent2)
        .ok_or_else(|| anyhow!("Can't find previously stored RootFsnodeId"))?
        .fsnode_id()
        .find_entry(
            ctx.clone(),
            hook_repo.repo_blobstore_arc(),
            path.clone().into(),
        )
        .await?;

    match (parent1_entry, parent2_entry) {
        (Some(p1_e), Some(p2_e)) => Ok(p1_e == p2_e),
        (None, None) => Ok(true),
        _ => {
            let commit_graph = hook_repo.commit_graph_arc();
            let first_lcs_cs_id = if let Some(lcs_cs_id) = commit_graph
                .common_base(ctx, parent1, parent2)
                .await?
                .first()
            {
                lcs_cs_id.clone()
            } else {
                return Ok(false);
            };

            let lca_root_fsnode_id = hook_repo
                .repo_derived_data()
                .derive::<RootFsnodeId>(ctx, first_lcs_cs_id)
                .await
                .with_context(|| "Can't lookup RootFsnodeId for ChangesetId")?;

            let lca_entry = lca_root_fsnode_id
                .fsnode_id()
                .find_entry(
                    ctx.clone(),
                    hook_repo.repo_blobstore_arc(),
                    path.clone().into(),
                )
                .await?;

            let non_empty_parent = parent1_entry
                .xor(parent2_entry)
                .ok_or_else(|| anyhow!("Exactly one parent has to be non-empty"))?;

            if let Some(lca_e) = lca_entry {
                Ok(lca_e == non_empty_parent)
            } else {
                Ok(false)
            }
        }
    }
}

async fn change_the_same_in_parent(
    ctx: &CoreContext,
    child_path: &NonRootMPath,
    child_file_change: &FileChange,
    parent_root_fsnode_id: &RootFsnodeId,
    hook_repo: &HookRepo,
) -> Result<bool> {
    let parent_entry = if let Some(entry) = parent_root_fsnode_id
        .fsnode_id()
        .find_entry(
            ctx.clone(),
            hook_repo.repo_blobstore_arc(),
            child_path.clone().into(),
        )
        .await?
    {
        entry
    } else {
        return Ok(false);
    };

    let parent_fsnode_file = if let Some(parent_fsnode_file) = parent_entry.into_leaf() {
        parent_fsnode_file
    } else {
        // In the child this was a file.
        return Ok(false);
    };

    if child_file_change.content_id() != Some(*parent_fsnode_file.content_id()) {
        return Ok(false);
    }

    Ok(true)
}
