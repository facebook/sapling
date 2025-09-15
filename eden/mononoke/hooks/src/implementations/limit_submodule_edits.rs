/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeSet;
use std::collections::HashSet;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use bookmarks::BookmarkKey;
use context::CoreContext;
use fsnodes::RootFsnodeId;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use manifest::ManifestOps;
use mononoke_types::BonsaiChangeset;
use mononoke_types::FileChange;
use mononoke_types::FileType;
use mononoke_types::path::MPath;
use regex::Regex;
use repo_blobstore::RepoBlobstoreArc;
use repo_derived_data::RepoDerivedDataRef;
use serde::Deserialize;

use crate::ChangesetHook;
use crate::CrossRepoPushSource;
use crate::HookConfig;
use crate::HookExecution;
use crate::HookRejectionInfo;
use crate::HookRepo;
use crate::PushAuthoredBy;

const NAMED_CAPTURE_NAME: &str = "marker_capture";
const MAX_CONCURRENCY: usize = 5;

#[derive(Clone, Debug, Deserialize)]
pub struct LimitSubmoduleEditsConfig {
    allow_edits_with_marker: Option<String>,
    disallow_new_submodules: Option<bool>,
}
#[derive(Clone, Debug)]
struct ChangesAllowedWithMarkerOptions {
    marker_extraction_regex: Regex,
    marker: String,
}

#[derive(Clone, Debug)]
pub struct LimitSubmoduleEditsHook {
    changes_allowed_with_marker_options: Option<ChangesAllowedWithMarkerOptions>,
    disallow_new_submodules: bool,
}

impl LimitSubmoduleEditsHook {
    pub fn new(config: &HookConfig) -> Result<Self> {
        Self::with_config(config.parse_options()?)
    }

    pub fn with_config(config: LimitSubmoduleEditsConfig) -> Result<Self> {
        let changes_allowed_with_marker_options =
            if let Some(marker) = config.allow_edits_with_marker {
                let marker_extraction_regex = Regex::new(&format!(
                    r"{}:\s*(?<{}>.+?)($|\n|\s)",
                    &marker, &NAMED_CAPTURE_NAME
                ))?;
                Some(ChangesAllowedWithMarkerOptions {
                    marker_extraction_regex,
                    marker,
                })
            } else {
                None
            };

        Ok(Self {
            changes_allowed_with_marker_options,
            disallow_new_submodules: config.disallow_new_submodules.unwrap_or(false),
        })
    }
}

fn get_submodule_mpaths(changeset: &BonsaiChangeset) -> BTreeSet<String> {
    changeset
        .file_changes()
        .filter_map(|(mpath, fc)| match fc {
            FileChange::Change(tfc) if tfc.file_type() == FileType::GitSubmodule => {
                Some(mpath.to_string())
            }
            _ => None,
        })
        .collect()
}

fn extract_paths_from_markers(
    options: &ChangesAllowedWithMarkerOptions,
    changeset: &BonsaiChangeset,
) -> BTreeSet<String> {
    options
        .marker_extraction_regex
        .captures_iter(changeset.message())
        .filter_map(|caps| caps.name(NAMED_CAPTURE_NAME).map(|m| m.as_str().to_owned()))
        .collect()
}

async fn get_new_submodule_mpaths(
    ctx: &CoreContext,
    hook_repo: &HookRepo,
    changeset: &BonsaiChangeset,
    submodule_paths: &BTreeSet<String>,
) -> Result<BTreeSet<String>> {
    let parent_root_fsnodes: &HashSet<RootFsnodeId> = &stream::iter(changeset.parents())
        .map(|p| async move {
            hook_repo
                .repo_derived_data()
                .derive::<RootFsnodeId>(ctx, p)
                .await
                .with_context(|| "Can't lookup RootFsnodeId for ChangesetId")
        })
        .buffer_unordered(MAX_CONCURRENCY)
        .try_collect::<HashSet<RootFsnodeId>>()
        .await?;

    let existing_submodule_paths: BTreeSet<String> = stream::iter(submodule_paths.iter().cloned())
        .map(|child_submodule_path| async move {
            for parent_root_fsnode in parent_root_fsnodes {
                let entry = parent_root_fsnode
                    .fsnode_id()
                    .find_entry(
                        ctx.clone(),
                        hook_repo.repo_blobstore_arc(),
                        MPath::new(&child_submodule_path)?,
                    )
                    .await?;
                if let Some(parent_entry) = entry {
                    if let Some(parent_fsnode_file) = parent_entry.into_leaf() {
                        if FileType::GitSubmodule == *parent_fsnode_file.file_type() {
                            return Ok(Some(child_submodule_path.clone()));
                        }
                    }
                }
            }
            Ok::<Option<String>, anyhow::Error>(None)
        })
        .buffer_unordered(MAX_CONCURRENCY)
        .try_filter_map(|x| async move { Ok(x) })
        .try_collect()
        .await?;

    Ok(submodule_paths
        .difference(&existing_submodule_paths)
        .cloned()
        .collect())
}

#[async_trait]
impl ChangesetHook for LimitSubmoduleEditsHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'repo: 'cs>(
        &'this self,
        ctx: &'ctx CoreContext,
        hook_repo: &'repo HookRepo,
        _bookmark: &BookmarkKey,
        changeset: &'cs BonsaiChangeset,
        _cross_repo_push_source: CrossRepoPushSource,
        _push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        let submodule_paths = get_submodule_mpaths(changeset);
        if submodule_paths.is_empty() {
            Ok(HookExecution::Accepted)
        } else {
            match &self.changes_allowed_with_marker_options {
                Some(options) => {
                    let allowlisted_submodule_paths =
                        extract_paths_from_markers(options, changeset);
                    let blocked_submodule_paths: BTreeSet<_> = submodule_paths
                        .difference(&allowlisted_submodule_paths)
                        .cloned()
                        .collect();
                    if blocked_submodule_paths.is_empty() {
                        if self.disallow_new_submodules {
                            let new_submodule_mpaths = get_new_submodule_mpaths(
                                ctx,
                                hook_repo,
                                changeset,
                                &submodule_paths,
                            )
                            .await?;
                            if new_submodule_mpaths.is_empty() {
                                Ok(HookExecution::Accepted)
                            } else {
                                return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                                    "New Git submodules are not allowed in this repository.",
                                    format!(
                                        "Commit creates submodules at the following paths:\n{}\n  This is disallowed even with correct markers in this repository.",
                                        new_submodule_mpaths
                                            .iter()
                                            .map(|submodule_path| format!(
                                                "    - {}",
                                                submodule_path
                                            ))
                                            .collect::<Vec<_>>()
                                            .join("\n"),
                                    ),
                                )));
                            }
                        } else {
                            Ok(HookExecution::Accepted)
                        }
                    } else {
                        Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                            "Changes to git submodules are restricted in this repository.",
                            format!(
                                "Commit creates or edits submodules at the following paths:\n{}\n  If you did mean to do this, add the following lines to your commit message:\n{}",
                                blocked_submodule_paths
                                    .iter()
                                    .map(|submodule_path| format!("    - {}", submodule_path))
                                    .collect::<Vec<_>>()
                                    .join("\n"),
                                blocked_submodule_paths
                                    .iter()
                                    .map(|submodule_path| format!(
                                        "  {}: {}",
                                        options.marker, submodule_path
                                    ))
                                    .collect::<Vec<_>>()
                                    .join("\n"),
                            ),
                        )))
                    }
                }
                None => Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                    "Git submodules or any changes to them are not allowed in this repository.",
                    format!(
                        "Commit creates or edits submodules at the following paths:\n{}",
                        submodule_paths
                            .iter()
                            .map(|submodule_path| format!("    - {}", submodule_path))
                            .collect::<Vec<_>>()
                            .join("\n"),
                    ),
                ))),
            }
        }
    }
}
