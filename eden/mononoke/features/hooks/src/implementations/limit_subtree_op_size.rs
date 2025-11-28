/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use blobstore::StoreLoadable;
use bookmarks::BookmarkKey;
use context::CoreContext;
use hook_manager::ChangesetHook;
use hook_manager::HookRejectionInfo;
use manifest::ManifestOps;
use mononoke_types::BonsaiChangeset;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use serde::Deserialize;
use skeleton_manifest::RootSkeletonManifestId;

use crate::CrossRepoPushSource;
use crate::HookConfig;
use crate::HookExecution;
use crate::HookRepo;
use crate::PushAuthoredBy;

#[derive(Deserialize, Clone, Debug)]
pub struct LimitSubtreeOpSizeConfig {
    #[serde(default)]
    source_file_count_limit: Option<u64>,

    #[serde(default)]
    dest_min_path_depth: Option<usize>,

    /// Message to use when rejecting a commit because the source directory is too large.
    ///
    /// The following variables are available:
    /// - ${dest_path}: The path of the destination.
    /// - ${source}: The changeset id of the source.
    /// - ${source_path}: The path of the source.
    /// - ${count}: The number of files in the source.
    /// - ${limit}: The limit on the number of files in the source.
    too_many_files_rejection_message: String,

    /// Message to use when rejecting a commit because the destination path is too shallow.
    ///
    /// The following variables are available:
    /// - ${dest_path}: The path of the destination.
    /// - ${dest_min_path_depth}: The minimum depth of the destination path.
    dest_too_shallow_rejection_message: String,
}

/// Hook to limit the size of subtree operations.
pub struct LimitSubtreeOpSizeHook {
    config: LimitSubtreeOpSizeConfig,
}

impl LimitSubtreeOpSizeHook {
    pub fn new(config: &HookConfig) -> Result<Self> {
        Self::with_config(config.parse_options()?)
    }

    pub fn with_config(config: LimitSubtreeOpSizeConfig) -> Result<Self> {
        Ok(Self { config })
    }
}

#[async_trait]
impl ChangesetHook for LimitSubtreeOpSizeHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'repo: 'cs>(
        &'this self,
        ctx: &'ctx CoreContext,
        hook_repo: &'repo HookRepo,
        _bookmark: &BookmarkKey,
        changeset: &'cs BonsaiChangeset,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution> {
        if push_authored_by.service() {
            return Ok(HookExecution::Accepted);
        }
        if cross_repo_push_source == CrossRepoPushSource::PushRedirected {
            // For push-redirected commits, we rely on running source-repo hooks
            return Ok(HookExecution::Accepted);
        }

        for (path, change) in changeset.subtree_changes() {
            if let Some(source_file_count_limit) = self.config.source_file_count_limit {
                if let Some((source, source_path)) = change.change_source() {
                    let source_root = hook_repo
                        .repo_derived_data()
                        .derive::<RootSkeletonManifestId>(ctx, source)
                        .await?
                        .into_skeleton_manifest_id();
                    let source_entry = source_root
                        .find_entry(
                            ctx.clone(),
                            hook_repo.repo_blobstore().clone(),
                            source_path.clone(),
                        )
                        .await?
                        .ok_or_else(|| {
                            anyhow::anyhow!(concat!(
                                "Source directory {source_path} ",
                                "not found in source changeset {source}"
                            ))
                        })?;
                    let source_count = match source_entry {
                        manifest::Entry::Tree(tree_id) => {
                            let source_tree = tree_id.load(ctx, hook_repo.repo_blobstore()).await?;
                            source_tree.summary().descendant_files_count
                        }
                        manifest::Entry::Leaf(_) => 1,
                    };

                    if source_count > source_file_count_limit {
                        return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                            "Subtree source is too large",
                            self.config
                                .too_many_files_rejection_message
                                .replace("${dest_path}", &path.to_string())
                                .replace("${source}", &source.to_string())
                                .replace("${source_path}", &source_path.to_string())
                                .replace("${count}", &source_count.to_string())
                                .replace("${limit}", &source_file_count_limit.to_string()),
                        )));
                    }
                }
            }

            if let Some(dest_min_path_depth) = self.config.dest_min_path_depth {
                if path.num_components() < dest_min_path_depth {
                    return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                        "Subtree destination is too shallow",
                        self.config
                            .dest_too_shallow_rejection_message
                            .replace("${dest_path}", &path.to_string())
                            .replace("${dest_min_path_depth}", &dest_min_path_depth.to_string()),
                    )));
                }
            }
        }

        Ok(HookExecution::Accepted)
    }
}
