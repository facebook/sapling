/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::format_err;
use async_trait::async_trait;
use bookmarks::BookmarkName;
use bytes::Bytes;
use changeset_info::ChangesetInfo;
use context::CoreContext;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::MPath;

use crate::ErrorKind;
use crate::FileChange;
use crate::FileContentManager;
use crate::PathContent;

#[derive(Clone)]
pub enum InMemoryFileText {
    Present(Bytes),
    Elided(u64),
}

impl From<Bytes> for InMemoryFileText {
    fn from(bytes: Bytes) -> InMemoryFileText {
        InMemoryFileText::Present(bytes)
    }
}

impl From<&str> for InMemoryFileText {
    fn from(string: &str) -> InMemoryFileText {
        let bytes: Bytes = Bytes::copy_from_slice(string.as_bytes());
        bytes.into()
    }
}

impl From<u64> for InMemoryFileText {
    fn from(int: u64) -> InMemoryFileText {
        InMemoryFileText::Elided(int)
    }
}

#[derive(Clone)]
pub struct InMemoryFileContentManager {
    id_to_text: HashMap<ContentId, InMemoryFileText>,
}

#[async_trait]
impl FileContentManager for InMemoryFileContentManager {
    async fn get_file_size<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        id: ContentId,
    ) -> Result<u64, ErrorKind> {
        self.id_to_text
            .get(&id)
            .ok_or(ErrorKind::ContentIdNotFound(id))
            .map(|maybe_bytes| match maybe_bytes {
                InMemoryFileText::Present(bytes) => bytes.len() as u64,
                InMemoryFileText::Elided(size) => *size,
            })
    }

    async fn get_file_text<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        id: ContentId,
    ) -> Result<Option<Bytes>, ErrorKind> {
        self.id_to_text
            .get(&id)
            .ok_or(ErrorKind::ContentIdNotFound(id))
            .map(|maybe_bytes| match maybe_bytes {
                InMemoryFileText::Present(bytes) => Some(bytes.clone()),
                InMemoryFileText::Elided(_) => None,
            })
    }

    async fn find_content<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        _bookmark: BookmarkName,
        _paths: Vec<MPath>,
    ) -> Result<HashMap<MPath, PathContent>, ErrorKind> {
        Err(
            format_err!("`find_content` is not implemented for `InMemoryFileContentManager`")
                .into(),
        )
    }

    async fn file_changes<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        _new_cs_id: ChangesetId,
        _old_cs_id: ChangesetId,
    ) -> Result<Vec<(MPath, FileChange)>, ErrorKind> {
        Err(
            format_err!("`file_changes` is not implemented for `InMemoryFileContentManager`")
                .into(),
        )
    }

    async fn latest_changes<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        _bookmark: BookmarkName,
        _paths: Vec<MPath>,
    ) -> Result<HashMap<MPath, ChangesetInfo>, ErrorKind> {
        Err(
            format_err!("`latest_changes` is not implemented for `InMemoryFileContentManager`")
                .into(),
        )
    }
}

impl InMemoryFileContentManager {
    pub fn new() -> InMemoryFileContentManager {
        InMemoryFileContentManager {
            id_to_text: HashMap::new(),
        }
    }

    pub fn insert(&mut self, key: ContentId, text: impl Into<InMemoryFileText>) {
        self.id_to_text.insert(key, text.into());
    }
}
