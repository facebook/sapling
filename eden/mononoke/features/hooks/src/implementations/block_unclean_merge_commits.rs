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
use content_manifest_derivation::RootContentManifestId;
use context::CoreContext;
use derivation_queue_thrift::DerivationPriority;
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
use mononoke_types::content_manifest::compat;
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

        let use_content_manifests = justknobs::eval(
            "scm/mononoke:derived_data_use_content_manifests",
            None,
            Some(hook_repo.repo_identity.name()),
        )?;
        let parent_root_manifests: HashMap<ChangesetId, compat::ContentManifestId> =
            stream::iter(changeset.parents().map(|p| {
                Ok(async move {
                    let root: compat::ContentManifestId = if use_content_manifests {
                        hook_repo
                            .repo_derived_data()
                            .derive::<RootContentManifestId>(ctx, p, DerivationPriority::LOW)
                            .await
                            .with_context(|| "Can't lookup RootContentManifestId for ChangesetId")?
                            .into_content_manifest_id()
                            .into()
                    } else {
                        hook_repo
                            .repo_derived_data()
                            .derive::<RootFsnodeId>(ctx, p, DerivationPriority::LOW)
                            .await
                            .with_context(|| "Can't lookup RootFsnodeId for ChangesetId")?
                            .into_fsnode_id()
                            .into()
                    };
                    Ok::<_, anyhow::Error>((p, root))
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
                &parent_root_manifests,
                use_content_manifests,
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
    parent_root_manifests: &HashMap<ChangesetId, compat::ContentManifestId>,
    use_content_manifests: bool,
) -> Result<bool> {
    match file_change {
        FileChange::Change(_) | FileChange::UntrackedChange(_) => {
            is_file_change_change_clean(
                ctx,
                path,
                file_change,
                changeset,
                hook_repo,
                parent_root_manifests,
            )
            .await
        }
        FileChange::Deletion | FileChange::UntrackedDeletion => {
            is_file_change_deletion_clean(
                ctx,
                path,
                changeset,
                hook_repo,
                parent_root_manifests,
                use_content_manifests,
            )
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
    parent_root_manifests: &HashMap<ChangesetId, compat::ContentManifestId>,
) -> Result<bool> {
    let mut parents_with_different_content = 0;
    for parent in changeset.parents() {
        let parent_root_manifest = parent_root_manifests
            .get(&parent)
            .ok_or_else(|| anyhow!("Can't find previously stored root manifest"))?;
        if !change_the_same_in_parent(ctx, path, file_change, parent_root_manifest, hook_repo)
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
    parent_root_manifests: &HashMap<ChangesetId, compat::ContentManifestId>,
    use_content_manifests: bool,
) -> Result<bool> {
    // Here, git straight up refuses to merge many branches if there are conflicts.
    if changeset.parents().count() > 2 {
        return Ok(false);
    }

    let (parent1, parent2) = changeset
        .parents()
        .collect_tuple()
        .ok_or_else(|| anyhow!("More than 2 expected parents"))?;

    let parent1_entry = parent_root_manifests
        .get(&parent1)
        .ok_or_else(|| anyhow!("Can't find previously stored root manifest"))?
        .find_entry(
            ctx.clone(),
            hook_repo.repo_blobstore_arc(),
            path.clone().into(),
        )
        .await?;

    let parent2_entry = parent_root_manifests
        .get(&parent2)
        .ok_or_else(|| anyhow!("Can't find previously stored root manifest"))?
        .find_entry(
            ctx.clone(),
            hook_repo.repo_blobstore_arc(),
            path.clone().into(),
        )
        .await?;

    match (parent1_entry.clone(), parent2_entry.clone()) {
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

            let lca_root_manifest: compat::ContentManifestId = if use_content_manifests {
                hook_repo
                    .repo_derived_data()
                    .derive::<RootContentManifestId>(ctx, first_lcs_cs_id, DerivationPriority::LOW)
                    .await
                    .with_context(|| "Can't lookup RootContentManifestId for ChangesetId")?
                    .into_content_manifest_id()
                    .into()
            } else {
                hook_repo
                    .repo_derived_data()
                    .derive::<RootFsnodeId>(ctx, first_lcs_cs_id, DerivationPriority::LOW)
                    .await
                    .with_context(|| "Can't lookup RootFsnodeId for ChangesetId")?
                    .into_fsnode_id()
                    .into()
            };

            let lca_entry = lca_root_manifest
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
    parent_root_manifest: &compat::ContentManifestId,
    hook_repo: &HookRepo,
) -> Result<bool> {
    let parent_entry = if let Some(entry) = parent_root_manifest
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

    let parent_manifest_file: compat::ContentManifestFile =
        if let Some(leaf) = parent_entry.into_leaf() {
            leaf.into()
        } else {
            // In the child this was a file.
            return Ok(false);
        };

    if child_file_change.content_id() != Some(parent_manifest_file.content_id()) {
        return Ok(false);
    }

    Ok(true)
}
