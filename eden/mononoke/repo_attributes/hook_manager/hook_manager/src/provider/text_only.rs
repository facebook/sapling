/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

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

use crate::errors::HookStateProviderError;
use crate::provider::BookmarkState;
use crate::provider::FileChangeType;
use crate::provider::HookStateProvider;
use crate::provider::PathContent;
use crate::provider::TagType;

pub struct TextOnlyHookStateProvider<T> {
    inner: Arc<T>,
    max_size: u64,
}

impl<T> TextOnlyHookStateProvider<T> {
    pub fn new(inner: T, max_size: u64) -> Self {
        Self {
            inner: Arc::new(inner),
            max_size,
        }
    }
}

#[async_trait]
impl<T: HookStateProvider + 'static> HookStateProvider for TextOnlyHookStateProvider<T> {
    async fn get_file_metadata<'a>(
        &'a self,
        ctx: &'a CoreContext,
        id: ContentId,
    ) -> Result<ContentMetadataV2, HookStateProviderError> {
        self.inner.get_file_metadata(ctx, id).await
    }

    /// Override the inner store's get_file_text by filtering out files that are to large or
    /// contain null bytes (those are assumed to be binary).
    async fn get_file_text<'a>(
        &'a self,
        ctx: &'a CoreContext,
        id: ContentId,
    ) -> Result<Option<Bytes>, HookStateProviderError> {
        // Don't fetch content if we know the object is too large
        let size = self.get_file_metadata(ctx, id).await?.total_size;
        if size > self.max_size {
            return Ok(None);
        }

        let file_bytes = self.inner.get_file_text(ctx, id).await?;

        Ok(file_bytes.filter(|bytes| !bytes.contains(&0)))
    }

    async fn find_content<'a>(
        &'a self,
        ctx: &'a CoreContext,
        bookmark: BookmarkKey,
        paths: Vec<NonRootMPath>,
    ) -> Result<HashMap<NonRootMPath, PathContent>, HookStateProviderError> {
        self.inner.find_content(ctx, bookmark, paths).await
    }

    async fn file_changes<'a>(
        &'a self,
        ctx: &'a CoreContext,
        new_cs_id: ChangesetId,
        old_cs_id: ChangesetId,
    ) -> Result<Vec<(NonRootMPath, FileChangeType)>, HookStateProviderError> {
        self.inner.file_changes(ctx, new_cs_id, old_cs_id).await
    }

    async fn latest_changes<'a>(
        &'a self,
        ctx: &'a CoreContext,
        bookmark: BookmarkKey,
        paths: Vec<NonRootMPath>,
    ) -> Result<HashMap<NonRootMPath, ChangesetInfo>, HookStateProviderError> {
        self.inner.latest_changes(ctx, bookmark, paths).await
    }

    async fn directory_sizes<'a>(
        &'a self,
        ctx: &'a CoreContext,
        changeset_id: ChangesetId,
        paths: Vec<MPath>,
    ) -> Result<HashMap<MPath, u64>, HookStateProviderError> {
        self.inner.directory_sizes(ctx, changeset_id, paths).await
    }

    async fn get_bookmark_state<'a, 'b>(
        &'a self,
        ctx: &'a CoreContext,
        bookmark: &'b BookmarkKey,
    ) -> Result<BookmarkState, HookStateProviderError> {
        self.inner.get_bookmark_state(ctx, bookmark).await
    }

    async fn get_tag_type<'a, 'b>(
        &'a self,
        ctx: &'a CoreContext,
        bookmark: &'b BookmarkKey,
    ) -> Result<TagType, HookStateProviderError> {
        self.inner.get_tag_type(ctx, bookmark).await
    }

    async fn get_git_commit<'a>(
        &'a self,
        ctx: &'a CoreContext,
        bonsai_commit_id: ChangesetId,
    ) -> Result<Option<GitSha1>, HookStateProviderError> {
        self.inner.get_git_commit(ctx, bonsai_commit_id).await
    }
}

#[cfg(test)]
mod test {
    use fbinit::FacebookInit;
    use mononoke_macros::mononoke;
    use mononoke_types_mocks::contentid::ONES_CTID;
    use tokio::runtime::Runtime;

    use super::*;
    use crate::InMemoryHookStateProvider;

    #[mononoke::fbinit_test]
    fn test_acceptable_file(fb: FacebookInit) {
        let rt = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock(fb);

        let mut inner = InMemoryHookStateProvider::new();
        inner.insert(ONES_CTID, "foobar");

        let store = TextOnlyHookStateProvider::new(inner, 10);
        let ret = rt.block_on(store.get_file_text(&ctx, ONES_CTID)).unwrap();
        assert_eq!(ret, Some("foobar".into()));
        let ret = rt
            .block_on(store.get_file_metadata(&ctx, ONES_CTID))
            .unwrap()
            .total_size;
        assert_eq!(ret, 6);
    }

    #[mononoke::fbinit_test]
    fn test_elide_large_file(fb: FacebookInit) {
        let rt = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock(fb);

        let mut inner = InMemoryHookStateProvider::new();
        inner.insert(ONES_CTID, "foobar");

        let store = TextOnlyHookStateProvider::new(inner, 2);
        let ret = rt.block_on(store.get_file_text(&ctx, ONES_CTID)).unwrap();
        assert_eq!(ret, None);

        let ret = rt
            .block_on(store.get_file_metadata(&ctx, ONES_CTID))
            .unwrap()
            .total_size;
        assert_eq!(ret, 6);
    }

    #[mononoke::fbinit_test]
    fn test_elide_binary_file(fb: FacebookInit) {
        let rt = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock(fb);

        let mut inner = InMemoryHookStateProvider::new();
        inner.insert(ONES_CTID, "foo\0");

        let store = TextOnlyHookStateProvider::new(inner, 10);
        let ret = rt.block_on(store.get_file_text(&ctx, ONES_CTID)).unwrap();
        assert_eq!(ret, None);
        let ret = rt
            .block_on(store.get_file_metadata(&ctx, ONES_CTID))
            .unwrap()
            .total_size;
        assert_eq!(ret, 4);
    }
}
