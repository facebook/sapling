/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::define_flags;
use super::Repo;
use super::Result;
use super::IO;
use clidispatch::errors;
use revisionstore::{
    CorruptionPolicy, DataPackStore, HgIdDataStore, IndexedLogHgIdDataStore, StoreKey, StoreResult,
    UnionHgIdDataStore,
};
use std::str::FromStr;
use types::{HgId, Key, RepoPathBuf};

define_flags! {
    pub struct DebugstoreOpts {
        /// print blob contents
        content: bool,

        #[arg]
        path: String,

        #[arg]
        hgid: String,
    }
}

pub fn run(opts: DebugstoreOpts, io: &mut IO, repo: Repo) -> Result<u8> {
    let path = RepoPathBuf::from_string(opts.path)?;
    let hgid = HgId::from_str(&opts.hgid)?;
    let config = repo.config();
    let cachepath = match config.get("remotefilelog", "cachepath") {
        Some(c) => c.to_string(),
        None => return Err(errors::Abort("remotefilelog.cachepath is not set".into()).into()),
    };
    let reponame = match config.get("remotefilelog", "reponame") {
        Some(c) => c.to_string(),
        None => return Err(errors::Abort("remotefilelog.reponame is not set".into()).into()),
    };
    let fullpath = format!("{}/{}/packs", cachepath, reponame);
    let packstore = Box::new(DataPackStore::new(fullpath, CorruptionPolicy::IGNORE));
    let fullpath = format!("{}/{}/indexedlogdatastore", cachepath, reponame);
    let indexedstore = Box::new(IndexedLogHgIdDataStore::new(fullpath).unwrap());
    let mut unionstore: UnionHgIdDataStore<Box<dyn HgIdDataStore>> = UnionHgIdDataStore::new();
    unionstore.add(packstore);
    unionstore.add(indexedstore);
    let k = Key::new(path, hgid);
    if let StoreResult::Found(content) = unionstore.get(StoreKey::hgid(k))? {
        io.write(content)?;
    }
    Ok(0)
}

pub fn name() -> &'static str {
    "debugstore"
}

pub fn doc() -> &'static str {
    "print information about blobstore"
}
