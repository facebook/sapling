/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::ErrorKind;
use crate::FileChange;
use crate::FileContentManager;
use crate::PathContent;

use async_trait::async_trait;
use bookmarks::BookmarkName;
use bytes::Bytes;
use changeset_info::ChangesetInfo;
use context::CoreContext;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::MPath;
use std::collections::HashMap;
use std::sync::Arc;

const NULL: u8 = 0;

pub struct TextOnlyFileContentManager<T> {
    inner: Arc<T>,
    max_size: u64,
}

impl<T> TextOnlyFileContentManager<T> {
    pub fn new(inner: T, max_size: u64) -> Self {
        Self {
            inner: Arc::new(inner),
            max_size,
        }
    }
}

#[async_trait]
impl<T: FileContentManager + 'static> FileContentManager for TextOnlyFileContentManager<T> {
    async fn get_file_size<'a>(
        &'a self,
        ctx: &'a CoreContext,
        id: ContentId,
    ) -> Result<u64, ErrorKind> {
        self.inner.get_file_size(ctx, id).await
    }

    /// Override the inner store's get_file_text by filtering out files that are to large or
    /// contain null bytes (those are assumed to be binary).
    async fn get_file_text<'a>(
        &'a self,
        ctx: &'a CoreContext,
        id: ContentId,
    ) -> Result<Option<Bytes>, ErrorKind> {
        // Don't fetch content if we know the object is too large
        let size = self.get_file_size(ctx, id).await?;
        if size > self.max_size {
            return Ok(None);
        }

        let file_bytes = self.inner.get_file_text(ctx, id).await?;

        Ok(file_bytes.and_then(|bytes| {
            if looks_like_binary(&bytes) {
                None
            } else {
                Some(bytes)
            }
        }))
    }

    async fn find_content<'a>(
        &'a self,
        ctx: &'a CoreContext,
        bookmark: BookmarkName,
        paths: Vec<MPath>,
    ) -> Result<HashMap<MPath, PathContent>, ErrorKind> {
        self.inner.find_content(ctx, bookmark, paths).await
    }

    async fn file_changes<'a>(
        &'a self,
        ctx: &'a CoreContext,
        new_cs_id: ChangesetId,
        old_cs_id: ChangesetId,
    ) -> Result<Vec<(MPath, FileChange)>, ErrorKind> {
        self.inner.file_changes(ctx, new_cs_id, old_cs_id).await
    }

    async fn latest_changes<'a>(
        &'a self,
        ctx: &'a CoreContext,
        bookmark: BookmarkName,
        paths: Vec<MPath>,
    ) -> Result<HashMap<MPath, ChangesetInfo>, ErrorKind> {
        self.inner.latest_changes(ctx, bookmark, paths).await
    }
}

fn looks_like_binary(file_bytes: &[u8]) -> bool {
    file_bytes.contains(&NULL)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::InMemoryFileContentManager;
    use fbinit::FacebookInit;
    use mononoke_types_mocks::contentid::ONES_CTID;
    use tokio::runtime::Runtime;

    #[fbinit::test]
    fn test_acceptable_file(fb: FacebookInit) {
        let rt = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock(fb);

        let mut inner = InMemoryFileContentManager::new();
        inner.insert(ONES_CTID, "foobar");

        let store = TextOnlyFileContentManager::new(inner, 10);
        let ret = rt.block_on(store.get_file_text(&ctx, ONES_CTID)).unwrap();
        assert_eq!(ret, Some("foobar".into()));
        let ret = rt.block_on(store.get_file_size(&ctx, ONES_CTID)).unwrap();
        assert_eq!(ret, 6);
    }

    #[fbinit::test]
    fn test_elide_large_file(fb: FacebookInit) {
        let rt = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock(fb);

        let mut inner = InMemoryFileContentManager::new();
        inner.insert(ONES_CTID, "foobar");

        let store = TextOnlyFileContentManager::new(inner, 2);
        let ret = rt.block_on(store.get_file_text(&ctx, ONES_CTID)).unwrap();
        assert_eq!(ret, None);

        let ret = rt.block_on(store.get_file_size(&ctx, ONES_CTID)).unwrap();
        assert_eq!(ret, 6);
    }

    #[fbinit::test]
    fn test_elide_binary_file(fb: FacebookInit) {
        let rt = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock(fb);

        let mut inner = InMemoryFileContentManager::new();
        inner.insert(ONES_CTID, "foo\0");

        let store = TextOnlyFileContentManager::new(inner, 10);
        let ret = rt.block_on(store.get_file_text(&ctx, ONES_CTID)).unwrap();
        assert_eq!(ret, None);
        let ret = rt.block_on(store.get_file_size(&ctx, ONES_CTID)).unwrap();
        assert_eq!(ret, 4);
    }
}
