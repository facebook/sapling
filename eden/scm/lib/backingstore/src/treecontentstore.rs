/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bytes::Bytes;
use failure::{format_err, Fallible};
use manifest::TreeStore;
use revisionstore::{ContentStore, DataStore};
use types::{HgId, Key, RepoPath};

pub(crate) struct TreeContentStore {
    inner: ContentStore,
}

impl TreeContentStore {
    pub fn new(inner: ContentStore) -> Self {
        TreeContentStore { inner }
    }
}

impl TreeStore for TreeContentStore {
    fn get(&self, path: &RepoPath, hgid: HgId) -> Fallible<Bytes> {
        let key = Key::new(path.to_owned(), hgid);

        self.inner.get(&key).and_then(|opt| {
            opt.ok_or_else(|| format_err!("hgid: {:?} path: {:?} is not found.", path, hgid))
                .map(Into::into)
        })
    }

    fn insert(&self, _path: &RepoPath, _hgid: HgId, _data: Bytes) -> Fallible<()> {
        Err(format_err!("insert is not implemented."))
    }
}
