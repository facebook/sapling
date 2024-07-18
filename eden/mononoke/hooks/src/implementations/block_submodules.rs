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
use mononoke_types::BonsaiChangeset;
use mononoke_types::FileChange;
use mononoke_types::FileType;
use mononoke_types::NonRootMPath;

use crate::ChangesetHook;
use crate::CrossRepoPushSource;
use crate::HookExecution;
use crate::HookFileContentProvider;
use crate::HookRejectionInfo;
use crate::PushAuthoredBy;

#[derive(Clone, Debug)]
pub struct BlockSubmodulesHook {}

impl BlockSubmodulesHook {
    pub fn new() -> Self {
        Self {}
    }
}

fn get_submodule_mpath(changeset: &BonsaiChangeset) -> Option<&NonRootMPath> {
    for (mpath, fc) in changeset.file_changes() {
        if let FileChange::Change(tfc) = fc {
            if tfc.file_type() == FileType::GitSubmodule {
                return Some(mpath);
            }
        }
    }
    None
}

#[async_trait]
impl ChangesetHook for BlockSubmodulesHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'fetcher: 'cs>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _bookmark: &BookmarkKey,
        changeset: &'cs BonsaiChangeset,
        _content_manager: &'fetcher dyn HookFileContentProvider,
        _cross_repo_push_source: CrossRepoPushSource,
        _push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        if let Some(submodule_mpath) = get_submodule_mpath(changeset) {
            return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                "Git submodules are not allowed in this repository.",
                format!("Commit introduces a submodule at path {}", submodule_mpath),
            )));
        }

        Ok(HookExecution::Accepted)
    }
}
