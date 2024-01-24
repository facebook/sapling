/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::anyhow;
use async_trait::async_trait;
use bookmarks_types::BookmarkKey;
use bytes::Bytes;
use changeset_info::ChangesetInfo;
use context::CoreContext;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::NonRootMPath;

use crate::errors::HookFileContentProviderError;
use crate::provider::FileChange;
use crate::provider::HookFileContentProvider;
use crate::provider::PathContent;

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
pub struct InMemoryHookFileContentProvider {
    id_to_text: HashMap<ContentId, InMemoryFileText>,
}

#[async_trait]
impl HookFileContentProvider for InMemoryHookFileContentProvider {
    async fn get_file_size<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        id: ContentId,
    ) -> Result<u64, HookFileContentProviderError> {
        self.id_to_text
            .get(&id)
            .ok_or(HookFileContentProviderError::ContentIdNotFound(id))
            .map(|maybe_bytes| match maybe_bytes {
                InMemoryFileText::Present(bytes) => bytes.len() as u64,
                InMemoryFileText::Elided(size) => *size,
            })
    }

    async fn get_file_text<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        id: ContentId,
    ) -> Result<Option<Bytes>, HookFileContentProviderError> {
        self.id_to_text
            .get(&id)
            .ok_or(HookFileContentProviderError::ContentIdNotFound(id))
            .map(|maybe_bytes| match maybe_bytes {
                InMemoryFileText::Present(bytes) => Some(bytes.clone()),
                InMemoryFileText::Elided(_) => None,
            })
    }

    async fn find_content<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        _bookmark: BookmarkKey,
        _paths: Vec<NonRootMPath>,
    ) -> Result<HashMap<NonRootMPath, PathContent>, HookFileContentProviderError> {
        Err(
            anyhow!("`find_content` is not implemented for `InMemoryHookFileContentProvider`")
                .into(),
        )
    }

    async fn file_changes<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        _new_cs_id: ChangesetId,
        _old_cs_id: ChangesetId,
    ) -> Result<Vec<(NonRootMPath, FileChange)>, HookFileContentProviderError> {
        Err(
            anyhow!("`file_changes` is not implemented for `InMemoryHookFileContentProvider`")
                .into(),
        )
    }

    async fn latest_changes<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        _bookmark: BookmarkKey,
        _paths: Vec<NonRootMPath>,
    ) -> Result<HashMap<NonRootMPath, ChangesetInfo>, HookFileContentProviderError> {
        Err(
            anyhow!("`latest_changes` is not implemented for `InMemoryHookFileContentProvider`")
                .into(),
        )
    }
}

impl InMemoryHookFileContentProvider {
    pub fn new() -> InMemoryHookFileContentProvider {
        InMemoryHookFileContentProvider {
            id_to_text: HashMap::new(),
        }
    }

    pub fn insert(&mut self, key: ContentId, text: impl Into<InMemoryFileText>) {
        self.id_to_text.insert(key, text.into());
    }
}
