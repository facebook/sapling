/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::{ErrorKind, FileContentFetcher};

use async_trait::async_trait;
use bytes::Bytes;
use context::CoreContext;
use mononoke_types::ContentId;
use std::sync::Arc;

const NULL: u8 = 0;

pub struct TextOnlyFileContentFetcher<T> {
    inner: Arc<T>,
    max_size: u64,
}

impl<T> TextOnlyFileContentFetcher<T> {
    pub fn new(inner: T, max_size: u64) -> Self {
        Self {
            inner: Arc::new(inner),
            max_size,
        }
    }
}

#[async_trait]
impl<T: FileContentFetcher + 'static> FileContentFetcher for TextOnlyFileContentFetcher<T> {
    async fn get_file_size<'a, 'b: 'a>(
        &'a self,
        ctx: &'b CoreContext,
        id: ContentId,
    ) -> Result<u64, ErrorKind> {
        self.inner.get_file_size(ctx, id).await
    }

    /// Override the inner store's get_file_text by filtering out files that are to large or
    /// contain null bytes (those are assumed to be binary).
    async fn get_file_text<'a, 'b: 'a>(
        &'a self,
        ctx: &'b CoreContext,
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
}

fn looks_like_binary(file_bytes: &[u8]) -> bool {
    file_bytes.contains(&NULL)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::InMemoryFileContentFetcher;
    use fbinit::FacebookInit;
    use mononoke_types_mocks::contentid::ONES_CTID;
    use tokio_compat::runtime::Runtime;

    #[fbinit::test]
    fn test_acceptable_file(fb: FacebookInit) {
        let mut rt = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock(fb);

        let mut inner = InMemoryFileContentFetcher::new();
        inner.insert(ONES_CTID, "foobar");

        let store = TextOnlyFileContentFetcher::new(inner, 10);
        let ret = rt
            .block_on_std(store.get_file_text(&ctx, ONES_CTID))
            .unwrap();
        assert_eq!(ret, Some("foobar".into()));
        let ret = rt
            .block_on_std(store.get_file_size(&ctx, ONES_CTID))
            .unwrap();
        assert_eq!(ret, 6);
    }

    #[fbinit::test]
    fn test_elide_large_file(fb: FacebookInit) {
        let mut rt = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock(fb);

        let mut inner = InMemoryFileContentFetcher::new();
        inner.insert(ONES_CTID, "foobar");

        let store = TextOnlyFileContentFetcher::new(inner, 2);
        let ret = rt
            .block_on_std(store.get_file_text(&ctx, ONES_CTID))
            .unwrap();
        assert_eq!(ret, None);

        let ret = rt
            .block_on_std(store.get_file_size(&ctx, ONES_CTID))
            .unwrap();
        assert_eq!(ret, 6);
    }

    #[fbinit::test]
    fn test_elide_binary_file(fb: FacebookInit) {
        let mut rt = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock(fb);

        let mut inner = InMemoryFileContentFetcher::new();
        inner.insert(ONES_CTID, "foo\0");

        let store = TextOnlyFileContentFetcher::new(inner, 10);
        let ret = rt
            .block_on_std(store.get_file_text(&ctx, ONES_CTID))
            .unwrap();
        assert_eq!(ret, None);
        let ret = rt
            .block_on_std(store.get_file_size(&ctx, ONES_CTID))
            .unwrap();
        assert_eq!(ret, 4);
    }
}
