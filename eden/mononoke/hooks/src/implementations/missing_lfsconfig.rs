/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use bookmarks::BookmarkKey;
use context::CoreContext;
use mononoke_types::BonsaiChangeset;
use mononoke_types::MPathElement;
use mononoke_types::NonRootMPath;

use crate::ChangesetHook;
use crate::CrossRepoPushSource;
use crate::HookExecution;
use crate::HookRejectionInfo;
use crate::HookStateProvider;
use crate::PathContent;
use crate::PushAuthoredBy;

#[derive(Clone, Debug)]
pub struct MissingLFSConfigHook {}

impl MissingLFSConfigHook {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl ChangesetHook for MissingLFSConfigHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'fetcher: 'cs>(
        &'this self,
        ctx: &'ctx CoreContext,
        bookmark: &BookmarkKey,
        changeset: &'cs BonsaiChangeset,
        content_manager: &'fetcher dyn HookStateProvider,
        _cross_repo_push_source: CrossRepoPushSource,
        _push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        let lfsconfig_path: NonRootMPath = MPathElement::new_from_slice(b".lfsconfig")?.into();

        let lfsconfig_file: HashMap<NonRootMPath, PathContent> = content_manager
            .find_content(ctx, bookmark.clone(), vec![lfsconfig_path.clone()])
            .await?;

        if !lfsconfig_file.is_empty() {
            return Ok(HookExecution::Accepted);
        }

        let adding_lfsconfig = changeset
            .file_changes()
            .any(|(path, _)| *path == lfsconfig_path);

        if adding_lfsconfig {
            return Ok(HookExecution::Accepted);
        }

        for (_, change) in changeset.file_changes() {
            if let Some(git_lfs) = change.git_lfs() {
                if git_lfs.is_lfs_pointer() {
                    return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                        "This commit add an LFS pointer but there is no .lfsconfig in this repository.",
                        "You need to add a properly defined .lfsconfig at the root directory of the repository prior to pushing.".to_string(),
                    )));
                }
            }
        }

        Ok(HookExecution::Accepted)
    }
}
