/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::{
    ChangesetHook, CrossRepoPushSource, FileContentFetcher, HookExecution, HookRejectionInfo,
};
use anyhow::Error;
use async_trait::async_trait;
use bookmarks::BookmarkName;
use context::CoreContext;
use mononoke_types::BonsaiChangeset;

#[derive(Clone, Debug)]
pub struct AlwaysFailChangeset;

impl AlwaysFailChangeset {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ChangesetHook for AlwaysFailChangeset {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'fetcher: 'cs>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _bookmark: &BookmarkName,
        _changeset: &'cs BonsaiChangeset,
        _content_fetcher: &'fetcher dyn FileContentFetcher,
        _cross_repo_push_source: CrossRepoPushSource,
    ) -> Result<HookExecution, Error> {
        Ok(HookExecution::Rejected(HookRejectionInfo::new(
            "This hook always fails",
        )))
    }
}
