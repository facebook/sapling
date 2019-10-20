// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use configparser::config::ConfigSet;
use configparser::hg::ConfigSetHgExt;
use failure::Fallible;
use revisionstore::{ContentStore, DataStore};
use std::path::Path;
use types::{Key, Node, RepoPath};

pub struct BackingStore {
    store: ContentStore,
}

impl BackingStore {
    pub fn new<P: AsRef<Path>>(repository: P) -> Fallible<Self> {
        let hg = repository.as_ref().join(".hg");
        let mut config = ConfigSet::new();
        config.load_system();
        config.load_user();
        config.load_hgrc(hg.join("hgrc"), "repository");

        let store = ContentStore::new(hg.join("store"), &config, None)?;

        Ok(Self { store })
    }

    pub fn get_blob(&self, path: &[u8], node: &[u8]) -> Fallible<Option<Vec<u8>>> {
        let path = RepoPath::from_utf8(path)?.to_owned();
        let node = Node::from_slice(node)?;
        let key = Key::new(path, node);

        self.store.get(&key)
    }
}
