/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use cloned::cloned;
use context::CoreContext;
use failure_ext::Error;
use futures::{Future, IntoFuture};
use futures_ext::{BoxFuture, FutureExt};
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

impl<T: FileContentStore + 'static> FileContentStore for TextOnlyFileContentStore<T> {
    fn resolve_path(
        &self,
        ctx: CoreContext,
        changeset_id: HgChangesetId,
        path: MPath,
    ) -> BoxFuture<Option<HgFileNodeId>, Error> {
        self.inner.resolve_path(ctx, changeset_id, path)
    }

    /// Override the inner store's get_file_text by filtering out files that are to large or
    /// contain null bytes (those are assumed to be binary).
    fn get_file_text(
        &self,
        ctx: CoreContext,
        id: HgFileNodeId,
    ) -> BoxFuture<Option<FileBytes>, Error> {
        self.get_file_size(ctx.clone(), id)
            .and_then({
                cloned!(self.inner, self.max_size);
                move |file_size| {
                    if file_size > max_size {
                        return Ok(None).into_future().left_future();
                    }

                    inner
                        .get_file_text(ctx, id)
                        .map(|file_bytes| match file_bytes {
                            Some(ref file_bytes) if looks_like_binary(&file_bytes) => None,
                            _ => file_bytes,
                        })
                        .right_future()
                }
            })
            .boxify()
    }

    fn get_file_size(&self, ctx: CoreContext, id: HgFileNodeId) -> BoxFuture<u64, Error> {
        self.inner.get_file_size(ctx, id)
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
    use tokio::runtime::Runtime;

    #[fbinit::test]
    fn test_acceptable_file(fb: FacebookInit) {
        let mut rt = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock(fb);

        let mut inner = InMemoryFileContentStore::new();
        inner.insert(ONES_CSID, MPath::new("f1").unwrap(), TWOS_FNID, "foobar");

        let store = TextOnlyFileContentStore::new(inner, 10);
        let ret = rt.block_on(store.get_file_text(ctx, TWOS_FNID)).unwrap();
        assert_eq!(ret, Some(FileBytes("foobar".into())));
    }

    #[fbinit::test]
    fn test_elide_large_file(fb: FacebookInit) {
        let mut rt = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock(fb);

        let mut inner = InMemoryFileContentStore::new();
        inner.insert(ONES_CSID, MPath::new("f1").unwrap(), TWOS_FNID, "foobar");

        let store = TextOnlyFileContentStore::new(inner, 2);
        let ret = rt.block_on(store.get_file_text(ctx, TWOS_FNID)).unwrap();
        assert_eq!(ret, None);
    }

    #[fbinit::test]
    fn test_elide_binary_file(fb: FacebookInit) {
        let mut rt = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock(fb);

        let mut inner = InMemoryFileContentStore::new();
        inner.insert(ONES_CSID, MPath::new("f1").unwrap(), TWOS_FNID, "foo\0");

        let store = TextOnlyFileContentStore::new(inner, 10);
        let ret = rt.block_on(store.get_file_text(ctx, TWOS_FNID)).unwrap();
        assert_eq!(ret, None);
    }
}
