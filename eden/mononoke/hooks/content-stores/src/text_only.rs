/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use async_trait::async_trait;
use context::CoreContext;
use mercurial_types::{FileBytes, HgChangesetId, HgFileNodeId, MPath};
use std::sync::Arc;

use crate::FileContentStore;

const NULL: u8 = 0;

pub struct TextOnlyFileContentStore<T> {
    inner: Arc<T>,
    max_size: u64,
}

impl<T> TextOnlyFileContentStore<T> {
    pub fn new(inner: T, max_size: u64) -> Self {
        Self {
            inner: Arc::new(inner),
            max_size,
        }
    }
}

#[async_trait]
impl<T: FileContentStore + 'static> FileContentStore for TextOnlyFileContentStore<T> {
    async fn resolve_path<'a, 'b: 'a>(
        &'a self,
        ctx: &'b CoreContext,
        changeset_id: HgChangesetId,
        path: MPath,
    ) -> Result<Option<HgFileNodeId>, Error> {
        self.inner.resolve_path(ctx, changeset_id, path).await
    }

    /// Override the inner store's get_file_text by filtering out files that are to large or
    /// contain null bytes (those are assumed to be binary).
    async fn get_file_text<'a, 'b: 'a>(
        &'a self,
        ctx: &'b CoreContext,
        id: HgFileNodeId,
    ) -> Result<Option<FileBytes>, Error> {
        let file_size = self.get_file_size(ctx, id).await?;
        if file_size > self.max_size {
            return Ok(None);
        }

        let file_bytes = self.inner.get_file_text(ctx, id).await?;
        Ok(match file_bytes {
            Some(ref file_bytes) if looks_like_binary(file_bytes) => None,
            _ => file_bytes,
        })
    }

    async fn get_file_size<'a, 'b: 'a>(
        &'a self,
        ctx: &'b CoreContext,
        id: HgFileNodeId,
    ) -> Result<u64, Error> {
        self.inner.get_file_size(ctx, id).await
    }
}

fn looks_like_binary(file_bytes: &FileBytes) -> bool {
    file_bytes.as_bytes().as_ref().contains(&NULL)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::InMemoryFileContentStore;
    use fbinit::FacebookInit;
    use mercurial_types_mocks::nodehash::{ONES_CSID, TWOS_FNID};
    use tokio_compat::runtime::Runtime;

    #[fbinit::test]
    fn test_acceptable_file(fb: FacebookInit) {
        let mut rt = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock(fb);

        let mut inner = InMemoryFileContentStore::new();
        inner.insert(ONES_CSID, MPath::new("f1").unwrap(), TWOS_FNID, "foobar");

        let store = TextOnlyFileContentStore::new(inner, 10);
        let ret = rt
            .block_on_std(store.get_file_text(&ctx, TWOS_FNID))
            .unwrap();
        assert_eq!(ret, Some(FileBytes("foobar".into())));
    }

    #[fbinit::test]
    fn test_elide_large_file(fb: FacebookInit) {
        let mut rt = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock(fb);

        let mut inner = InMemoryFileContentStore::new();
        inner.insert(ONES_CSID, MPath::new("f1").unwrap(), TWOS_FNID, "foobar");

        let store = TextOnlyFileContentStore::new(inner, 2);
        let ret = rt
            .block_on_std(store.get_file_text(&ctx, TWOS_FNID))
            .unwrap();
        assert_eq!(ret, None);
    }

    #[fbinit::test]
    fn test_elide_binary_file(fb: FacebookInit) {
        let mut rt = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock(fb);

        let mut inner = InMemoryFileContentStore::new();
        inner.insert(ONES_CSID, MPath::new("f1").unwrap(), TWOS_FNID, "foo\0");

        let store = TextOnlyFileContentStore::new(inner, 10);
        let ret = rt
            .block_on_std(store.get_file_text(&ctx, TWOS_FNID))
            .unwrap();
        assert_eq!(ret, None);
    }
}
