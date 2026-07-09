/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Implement traits from other crates.

use async_trait::async_trait;
use blob::Blob;
use storemodel::ContentFetchItems;
use storemodel::FileStore;
use storemodel::InsertOpts;
use storemodel::KeyStore;
use storemodel::Kind;
use storemodel::SerializationFormat;
use storemodel::TreeFetchItems;
use storemodel::TreeStore;
use types::FetchContext;
use types::HgId;
use types::Key;
use types::RepoPath;
use types::fetch_mode::FetchMode;

use crate::GitStore;
use crate::ObjectType;

#[async_trait]
impl KeyStore for GitStore {
    fn get_local_content(&self, _path: &RepoPath, id: HgId) -> anyhow::Result<Option<Blob>> {
        match self.read_obj(id, ObjectType::Any, FetchMode::LocalOnly) {
            Ok(data) => Ok(Some(Blob::Bytes(data.into()))),
            Err(e) => {
                if let Some(e) = e.downcast_ref::<git2::Error>() {
                    if e.code() == git2::ErrorCode::NotFound {
                        return Ok(None);
                    }
                }
                Err(e)
            }
        }
    }

    fn get_content_iter(
        &self,
        fctx: FetchContext,
        keys: Vec<types::Key>,
    ) -> anyhow::Result<ContentFetchItems> {
        let fetch_mode = fctx.mode();
        if self.has_fetch_url() && fetch_mode.contains(FetchMode::REMOTE) {
            let ids = keys.iter().map(|k| k.hgid).collect::<Vec<_>>();
            self.fetch_objs(&ids)?
        }
        if fetch_mode.contains(FetchMode::IGNORE_RESULT) {
            return Ok(ContentFetchItems::empty());
        }
        let store = self.clone();
        let iter = keys.into_iter().map(move |k| {
            // Use LocalOnly since bulk fetch_objs above already fetched from remote.
            let data = store.read_obj(k.hgid, ObjectType::Any, FetchMode::LocalOnly)?;
            Ok((k, Blob::Bytes(data.into())))
        });
        Ok(ContentFetchItems::item_stream(iter))
    }

    // This is an old API but still critical for BFS tree fetching.
    fn prefetch(&self, keys: Vec<types::Key>) -> anyhow::Result<()> {
        if self.has_fetch_url() {
            let ids = keys.iter().map(|k| k.hgid).collect::<Vec<_>>();
            self.fetch_objs(&ids)?;
        }
        Ok(())
    }

    fn insert_data(&self, opts: InsertOpts, path: &RepoPath, data: Blob) -> anyhow::Result<HgId> {
        let kind = match opts.kind {
            Kind::File => ObjectType::Blob,
            Kind::Tree => ObjectType::Tree,
        };
        let data = data.to_bytes();
        let id = self.write_obj(kind, &data)?;
        if let Some(forced_id) = opts.forced_id {
            if forced_id.as_ref() != &id {
                anyhow::bail!("hash mismatch when writing {path}@{forced_id}");
            }
        }
        Ok(id)
    }

    fn format(&self) -> SerializationFormat {
        SerializationFormat::Git
    }

    fn sync(&self) -> anyhow::Result<()> {
        // We don't hold state in memory, so no need to sync.
        Ok(())
    }

    fn flush(&self) -> anyhow::Result<()> {
        // We don't hold pending state in memory, so no need to flush.
        Ok(())
    }

    fn clone_key_store(&self) -> Box<dyn KeyStore> {
        Box::new(self.clone())
    }
}

#[async_trait]
impl FileStore for GitStore {
    fn clone_file_store(&self) -> Box<dyn FileStore> {
        Box::new(self.clone())
    }
}

impl TreeStore for GitStore {
    fn clone_tree_store(&self) -> Box<dyn TreeStore> {
        Box::new(self.clone())
    }

    fn get_tree_iter(&self, _fctx: FetchContext, keys: Vec<Key>) -> anyhow::Result<TreeFetchItems> {
        // Bulk fetch from remote first.
        if self.has_fetch_url() {
            let ids = keys.iter().map(|k| k.hgid).collect::<Vec<_>>();
            self.fetch_objs(&ids)?;
        }
        // Then read locally and parse into TreeEntry.
        let store = self.clone_tree_store();
        let iter = keys
            .into_iter()
            .map(move |k| match store.get_local_tree(&k.path, k.hgid) {
                Err(e) => Err(e),
                Ok(None) => Err(anyhow::format_err!(
                    "{}@{}: not found locally",
                    k.path,
                    k.hgid
                )),
                Ok(Some(data)) => Ok((k, data)),
            });
        Ok(TreeFetchItems::item_stream(iter))
    }
}
