/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::format_err;
use anyhow::Result;
use manifest_tree::TreeStore;
use minibytes::Bytes;
use revisionstore::ContentStore;
use revisionstore::HgIdDataStore;
use revisionstore::StoreKey;
use revisionstore::StoreResult;
use types::HgId;
use types::Key;
use types::RepoPath;

pub(crate) struct TreeContentStore {
    inner: ContentStore,
}

impl TreeContentStore {
    pub fn new(inner: ContentStore) -> Self {
        TreeContentStore { inner }
    }

    pub fn as_content_store(&self) -> &ContentStore {
        &self.inner
    }
}

impl TreeStore for TreeContentStore {
    fn get(&self, path: &RepoPath, hgid: HgId) -> Result<Bytes> {
        let key = StoreKey::hgid(Key::new(path.to_owned(), hgid));

        self.inner.get(key).and_then(|res| match res {
            StoreResult::Found(data) => Ok(data.into()),
            StoreResult::NotFound(_) => Err(format_err!(
                "hgid: {:?} path: {:?} is not found.",
                hgid,
                path
            )),
        })
    }

    fn insert(&self, _path: &RepoPath, _hgid: HgId, _data: Bytes) -> Result<()> {
        Err(format_err!("insert is not implemented."))
    }
}
