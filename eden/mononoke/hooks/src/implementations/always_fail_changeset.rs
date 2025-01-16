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
use serde::Deserialize;

use crate::ChangesetHook;
use crate::CrossRepoPushSource;
use crate::HookExecution;
use crate::HookRejectionInfo;
use crate::HookRepo;
use crate::PushAuthoredBy;

#[derive(Debug, Deserialize, Clone, Default)]
pub struct AlwaysFailChangesetConfig {
    /// Optional message to display when the hook fails.
    message: Option<String>,
}

#[derive(Clone, Debug)]
pub struct AlwaysFailChangeset {
    config: AlwaysFailChangesetConfig,
}

impl AlwaysFailChangeset {
    pub fn new(config: &HookConfig) -> Result<Self> {
        let config = config.parse_options().unwrap_or_default();
        Ok(Self::with_config(config))
    }

    pub fn with_config(config: AlwaysFailChangesetConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl ChangesetHook for AlwaysFailChangeset {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'repo: 'cs>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _hook_repo: &'repo HookRepo,
        _bookmark: &BookmarkKey,
        _changeset: &'cs BonsaiChangeset,
        _cross_repo_push_source: CrossRepoPushSource,
        _push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        let msg = self
            .config
            .message
            .clone()
            .unwrap_or("This hook always fails".to_string());
        Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
            "Always fail hook",
            msg,
        )))
    }
}
