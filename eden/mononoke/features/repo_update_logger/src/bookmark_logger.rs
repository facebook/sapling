/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;

#[cfg(fbcode_build)]
use anyhow::Result;
use async_trait::async_trait;
use bookmarks::BookmarkUpdateReason;
use bookmarks_types::BookmarkKind;
use bookmarks_types::BookmarkName;
use context::CoreContext;
use logger_ext::Loggable;
use metaconfig_types::RepoConfigRef;
#[cfg(fbcode_build)]
use mononoke_bookmark_rust_logger::MononokeBookmarkLogger;
use mononoke_types::ChangesetId;
use repo_identity::RepoIdentityRef;
use serde_derive::Serialize;

#[derive(Serialize)]
pub enum BookmarkOperation {
    Create(ChangesetId),
    Update(ChangesetId, ChangesetId),
    Pushrebase(Option<ChangesetId>, ChangesetId),
    Delete(ChangesetId),
}

impl std::fmt::Display for BookmarkOperation {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use BookmarkOperation::*;

        let s = match self {
            Create(_) => "create",
            Update(_, _) => "update",
            Pushrebase(_, _) => "pushrebase",
            Delete(_) => "delete",
        };

        write!(f, "{}", s)
    }
}

impl BookmarkOperation {
    fn old_bookmark_value(&self) -> Option<ChangesetId> {
        use BookmarkOperation::*;
        match *self {
            Create(_) => None,
            Update(old, _) => Some(old),
            Pushrebase(old, _) => old,
            Delete(old) => Some(old),
        }
    }
    fn new_bookmark_value(&self) -> Option<ChangesetId> {
        use BookmarkOperation::*;
        match *self {
            Create(new) => Some(new),
            Update(_, new) => Some(new),
            Pushrebase(_, new) => Some(new),
            Delete(_) => None,
        }
    }
}

pub struct BookmarkInfo {
    pub bookmark_name: BookmarkName,
    pub bookmark_kind: BookmarkKind,
    pub operation: BookmarkOperation,
    pub reason: BookmarkUpdateReason,
}

#[derive(Serialize)]
struct PlainBookmarkInfo {
    repo_name: String,
    bookmark_name: String,
    bookmark_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    old_bookmark_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    new_bookmark_value: Option<String>,
    operation: String,
    update_reason: String,
}

impl PlainBookmarkInfo {
    fn new(repo: &impl RepoIdentityRef, info: &BookmarkInfo) -> Self {
        Self {
            repo_name: repo.repo_identity().name().to_owned(),
            bookmark_name: format!("{}", info.bookmark_name),
            bookmark_kind: format!("{}", info.bookmark_kind),
            old_bookmark_value: info
                .operation
                .old_bookmark_value()
                .map(|cs| format!("{}", cs)),
            new_bookmark_value: info
                .operation
                .new_bookmark_value()
                .map(|cs| format!("{}", cs)),
            operation: format!("{}", info.operation),
            update_reason: format!("{}", info.reason),
        }
    }
}

#[async_trait]
impl Loggable for PlainBookmarkInfo {
    #[cfg(fbcode_build)]
    async fn log_to_logger(&self, ctx: &CoreContext) -> Result<()> {
        let mut logger = MononokeBookmarkLogger::new(ctx.fb);
        logger
            .set_repo_name(self.repo_name.clone())
            .set_bookmark_name(self.bookmark_name.clone())
            .set_bookmark_kind(self.bookmark_kind.clone());
        if let Some(v) = &self.old_bookmark_value {
            logger.set_old_bookmark_value(v.clone());
        }
        if let Some(v) = &self.new_bookmark_value {
            logger.set_new_bookmark_value(v.clone());
        }
        logger
            .set_operation(self.operation.clone())
            .set_update_reason(self.update_reason.clone());

        logger.log_async()?;

        Ok(())
    }
}

pub async fn log_bookmark_operation(
    ctx: &CoreContext,
    repo: &(impl RepoIdentityRef + RepoConfigRef),
    info: &BookmarkInfo,
) {
    if let Some(bookmark_logging_destination) = &repo
        .repo_config()
        .update_logging_config
        .bookmark_logging_destination
    {
        PlainBookmarkInfo::new(repo, info)
            .log(ctx, bookmark_logging_destination)
            .await;
    }
}
