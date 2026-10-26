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
use derivation_queue_thrift::DerivationPriority;
use hook_manager::ChangesetHook;
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
use crate::Pushvars;
use crate::hook_messages::HookMessages;
use crate::hook_messages::hook_messages;

hook_messages! {
    /// Messages of the `limit_subtree_op_size` hook.
    pub(crate) struct LimitSubtreeOpSizeMessages for "limit_subtree_op_size" {
        /// The source of a subtree copy or merge has more files than
        /// `source_file_count_limit`.
        too_many_files(
            /// Path of the destination.
            dest_path,
            /// The changeset the source comes from.
            source,
            /// Path of the source.
            source_path,
            /// Number of files in the source.
            count,
            /// The limit on the number of files in the source.
            limit,
        ) = "Subtree source ${source_path} in ${source} has ${count} files but at most ${limit} are allowed (destination ${dest_path}).";
        /// The destination of a subtree operation is shallower than
        /// `dest_min_path_depth`.
        dest_too_shallow(
            /// Path of the destination.
            dest_path,
            /// The minimum number of path components of the destination.
            dest_min_path_depth,
        ) = "Subtree destination ${dest_path} must have at least ${dest_min_path_depth} path components.";
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct LimitSubtreeOpSizeConfig {
    #[serde(default)]
    source_file_count_limit: Option<u64>,

    #[serde(default)]
    dest_min_path_depth: Option<usize>,

    /// Replaces the `too_many_files` message. Use the hook config's
    /// `messages` instead.
    #[serde(default)]
    too_many_files_rejection_message: Option<String>,

    /// Replaces the `dest_too_shallow` message. Use the hook config's
    /// `messages` instead.
    #[serde(default)]
    dest_too_shallow_rejection_message: Option<String>,
}

/// Hook to limit the size of subtree operations.
pub struct LimitSubtreeOpSizeHook {
    config: LimitSubtreeOpSizeConfig,
    messages: LimitSubtreeOpSizeMessages,
}

impl LimitSubtreeOpSizeHook {
    pub fn new(config: &HookConfig) -> Result<Self> {
        let options: LimitSubtreeOpSizeConfig = config.parse_options()?;
        let messages = LimitSubtreeOpSizeMessages::builder()
            .legacy(
                "too_many_files",
                options.too_many_files_rejection_message.as_deref(),
            )
            .legacy(
                "dest_too_shallow",
                options.dest_too_shallow_rejection_message.as_deref(),
            )
            .build(config);
        Ok(Self {
            config: options,
            messages,
        })
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
        _maybe_pushvars: Option<&'cs Pushvars>,
    ) -> Result<HookExecution> {
        if push_authored_by.service() {
            return Ok(HookExecution::accepted());
        }
        if cross_repo_push_source == CrossRepoPushSource::PushRedirected {
            // For push-redirected commits, we rely on running source-repo hooks
            return Ok(HookExecution::accepted());
        }

        for (path, change) in changeset.subtree_changes() {
            if let Some(source_file_count_limit) = self.config.source_file_count_limit {
                if let Some((source, source_path)) = change.change_source() {
                    let source_root = hook_repo
                        .repo_derived_data()
                        .derive::<RootSkeletonManifestId>(ctx, source, DerivationPriority::LOW)
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
                        return Ok(HookExecution::rejected(
                            self.messages
                                .too_many_files(
                                    path,
                                    source,
                                    source_path,
                                    source_count,
                                    source_file_count_limit,
                                )
                                .into_rejection("Subtree source is too large"),
                        ));
                    }
                }
            }

            if let Some(dest_min_path_depth) = self.config.dest_min_path_depth {
                if path.num_components() < dest_min_path_depth {
                    return Ok(HookExecution::rejected(
                        self.messages
                            .dest_too_shallow(path, dest_min_path_depth)
                            .into_rejection("Subtree destination is too shallow"),
                    ));
                }
            }
        }

        Ok(HookExecution::accepted())
    }
}
