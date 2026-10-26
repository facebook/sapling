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
use crate::HookRepo;
use crate::PushAuthoredBy;
use crate::Pushvars;
use crate::hook_messages::HookMessages;
use crate::hook_messages::hook_messages;

hook_messages! {
    /// Messages of the `always_fail_changeset` hook.
    pub(crate) struct AlwaysFailChangesetMessages for "always_fail_changeset" {
        /// Shown for every commit: this hook rejects them all.
        failed() = "This hook always fails";
    }
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct AlwaysFailChangesetConfig {
    /// Replaces the `failed` message. Use the hook config's `messages` instead.
    message: Option<String>,
}

#[derive(Clone, Debug)]
pub struct AlwaysFailChangeset {
    messages: AlwaysFailChangesetMessages,
}

impl AlwaysFailChangeset {
    pub fn new(config: &HookConfig) -> Result<Self> {
        let options: AlwaysFailChangesetConfig = config.parse_options().unwrap_or_default();
        let messages = AlwaysFailChangesetMessages::builder()
            .legacy("failed", options.message.as_deref())
            .build(config);
        Ok(Self { messages })
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
        _maybe_pushvars: Option<&'cs Pushvars>,
    ) -> Result<HookExecution, Error> {
        Ok(HookExecution::rejected(
            self.messages.failed().into_rejection("Always fail hook"),
        ))
    }
}
