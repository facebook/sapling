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
use mononoke_types::hash::GitSha1;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::ContentMetadataV2;
use mononoke_types::MPath;
use mononoke_types::NonRootMPath;
use quickcheck::Arbitrary;
use quickcheck::Gen;

use crate::errors::HookStateProviderError;
use crate::provider::BookmarkState;
use crate::provider::FileChangeType;
use crate::provider::HookStateProvider;
use crate::provider::PathContent;
use crate::provider::TagType;

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
pub struct InMemoryHookStateProvider {
    id_to_text: HashMap<ContentId, InMemoryFileText>,
}

#[async_trait]
impl HookStateProvider for InMemoryHookStateProvider {
    async fn get_file_metadata<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        id: ContentId,
    ) -> Result<ContentMetadataV2, HookStateProviderError> {
        let mb_content_md = self
            .id_to_text
            .get(&id)
            .map(|maybe_bytes| match maybe_bytes {
                InMemoryFileText::Present(bytes) => Some(ContentMetadataV2 {
                    total_size: bytes.len() as u64,
                    ..ContentMetadataV2::arbitrary(&mut Gen::new(100))
                }),
                InMemoryFileText::Elided(size) => Some(ContentMetadataV2 {
                    total_size: *size,
                    ..ContentMetadataV2::arbitrary(&mut Gen::new(100))
                }),
            });
        mb_content_md
            .flatten()
            .ok_or(HookStateProviderError::ContentIdNotFound(id))
    }

    async fn get_file_text<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        id: ContentId,
    ) -> Result<Option<Bytes>, HookStateProviderError> {
        self.id_to_text
            .get(&id)
            .ok_or(HookStateProviderError::ContentIdNotFound(id))
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
    ) -> Result<HashMap<NonRootMPath, PathContent>, HookStateProviderError> {
        Err(anyhow!("`find_content` is not implemented for `InMemoryHookStateProvider`").into())
    }

    async fn file_changes<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        _new_cs_id: ChangesetId,
        _old_cs_id: ChangesetId,
    ) -> Result<Vec<(NonRootMPath, FileChangeType)>, HookStateProviderError> {
        Err(anyhow!("`file_changes` is not implemented for `InMemoryHookStateProvider`").into())
    }

    async fn latest_changes<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        _bookmark: BookmarkKey,
        _paths: Vec<NonRootMPath>,
    ) -> Result<HashMap<NonRootMPath, ChangesetInfo>, HookStateProviderError> {
        Err(anyhow!("`latest_changes` is not implemented for `InMemoryHookStateProvider`").into())
    }

    async fn directory_sizes<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        _changeset_id: ChangesetId,
        _paths: Vec<MPath>,
    ) -> Result<HashMap<MPath, u64>, HookStateProviderError> {
        Err(anyhow!("`directory_sizes` is not implemented for `InMemoryHookStateProvider`").into())
    }

    async fn get_bookmark_state<'a, 'b>(
        &'a self,
        _ctx: &'a CoreContext,
        _bookmark: &'b BookmarkKey,
    ) -> Result<BookmarkState, HookStateProviderError> {
        Err(
            anyhow!("`get_bookmark_state` is not implemented for `InMemoryHookStateProvider`")
                .into(),
        )
    }

    async fn get_tag_type<'a, 'b>(
        &'a self,
        _ctx: &'a CoreContext,
        _bookmark: &'b BookmarkKey,
    ) -> Result<TagType, HookStateProviderError> {
        Err(anyhow!("`get_tag_state` is not implemented for `InMemoryHookStateProvider`").into())
    }

    async fn get_git_commit<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        _bonsai_commit_id: ChangesetId,
    ) -> Result<Option<GitSha1>, HookStateProviderError> {
        Err(anyhow!("`get_git_commit` is not implemented for `InMemoryHookStateProvider`").into())
    }
}

impl InMemoryHookStateProvider {
    pub fn new() -> InMemoryHookStateProvider {
        InMemoryHookStateProvider {
            id_to_text: HashMap::new(),
        }
    }

    pub fn insert(&mut self, key: ContentId, text: impl Into<InMemoryFileText>) {
        self.id_to_text.insert(key, text.into());
    }
}
