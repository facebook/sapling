// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use configparser::config::ConfigSet;
use configparser::hg::ConfigSetHgExt;
use failure::Fallible;
use revisionstore::ContentStore;
use std::path::Path;

pub struct BackingStore {
    #[allow(dead_code)]
    store: ContentStore,
}

impl BackingStore {
    pub fn new<P: AsRef<Path>>(repository: P) -> Fallible<Self> {
        let mut config = ConfigSet::new();
        config.load_system();
        config.load_user();
        config.load_hgrc(repository.as_ref().join(".hg").join("hgrc"), "repository");

        let store = ContentStore::new(repository, &config, None)?;

        Ok(Self { store })
    }
}
