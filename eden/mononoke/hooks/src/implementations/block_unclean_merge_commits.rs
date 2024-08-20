/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use bookmarks::BookmarkKey;
use context::CoreContext;
use metaconfig_types::HookConfig;
use mononoke_types::BonsaiChangeset;
use regex::Regex;
use serde::Deserialize;

use crate::ChangesetHook;
use crate::CrossRepoPushSource;
use crate::HookExecution;
use crate::HookRejectionInfo;
use crate::HookStateProvider;
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
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'fetcher: 'cs>(
        &'this self,
        _ctx: &'ctx CoreContext,
        bookmark: &BookmarkKey,
        changeset: &'cs BonsaiChangeset,
        _content_manager: &'fetcher dyn HookStateProvider,
        _cross_repo_push_source: CrossRepoPushSource,
        _push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        if bookmark.is_tag() {
            return Ok(HookExecution::Accepted);
        }

        if !changeset.is_merge() {
            return Ok(HookExecution::Accepted);
        }

        if changeset.file_changes().next().is_none() {
            return Ok(HookExecution::Accepted);
        }

        if let Some(regex) = &self.config.only_check_branches_matching_regex {
            if !regex.is_match(bookmark.as_str()) {
                Ok(HookExecution::Accepted)
            } else {
                Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                    "Merge commits with conflicts, even if resolved are not allowed.",
                    format!(
                        "The bookmark matching regex {} can't have merge commits with conflicts, even if they have been resolved",
                        regex
                    ),
                )))
            }
        } else {
            Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                "Merge commits with conflicts, even if resolved are not allowed.",
                "The repo can't have merge commits with conflicts, even if they have been resolved"
                    .to_string(),
            )))
        }
    }
}
