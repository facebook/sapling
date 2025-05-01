/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Implement traits defined by other crates.

use std::sync::Arc;

use anyhow::Result;
use blob::Blob;
use edenapi_types::FileAuxData;
use format_util::git_sha1_digest;
use format_util::hg_sha1_digest;
use minibytes::Bytes;
use storemodel::BoxIterator;
use storemodel::InsertOpts;
use storemodel::KeyStore;
use storemodel::Kind;
use storemodel::SerializationFormat;
use types::FetchContext;
use types::HgId;
use types::Id20;
use types::Key;
use types::RepoPath;
use types::hgid::NULL_ID;

use crate::scmstore::FileAttributes;
use crate::scmstore::FileStore;

// Wrapper types to workaround Rust's orphan rule.
#[derive(Clone)]
pub struct ArcFileStore(pub Arc<FileStore>);

impl storemodel::KeyStore for ArcFileStore {
    fn get_content_iter(
        &self,
        fctx: FetchContext,
        keys: Vec<Key>,
    ) -> anyhow::Result<BoxIterator<anyhow::Result<(Key, Blob)>>> {
        let fetched = self.0.fetch(fctx, keys, FileAttributes::PURE_CONTENT);
        let iter = fetched
            .into_iter()
            .map(|result| -> anyhow::Result<(Key, Blob)> {
                let (key, store_file) = result?;
                let content = store_file.file_content()?;
                Ok((key, content))
            });
        Ok(Box::new(iter))
    }

    fn get_local_content(&self, _path: &RepoPath, hgid: HgId) -> anyhow::Result<Option<Blob>> {
        self.0.get_local_content_direct(&hgid)
    }

    fn flush(&self) -> Result<()> {
        FileStore::flush(&self.0)
    }

    fn refresh(&self) -> Result<()> {
        FileStore::refresh(&self.0)
    }

    fn statistics(&self) -> Vec<(String, usize)> {
        FileStore::metrics(&self.0)
    }

    /// Decides whether the store uses git or hg format.
    fn format(&self) -> SerializationFormat {
        self.0.format
    }

    fn insert_data(&self, opts: InsertOpts, path: &RepoPath, data: &[u8]) -> anyhow::Result<HgId> {
        let id = sha1_digest(&opts, data, self.format());
        let key = Key::new(path.to_owned(), id);
        // PERF: Ideally, there is no need to copy `data`.
        let data = Bytes::copy_from_slice(data);
        self.0.write_nonlfs(key, data, Default::default())?;
        Ok(id)
    }

    fn clone_key_store(&self) -> Box<dyn KeyStore> {
        Box::new(self.clone())
    }
}

impl storemodel::FileStore for ArcFileStore {
    fn get_rename_iter(
        &self,
        keys: Vec<Key>,
    ) -> anyhow::Result<BoxIterator<anyhow::Result<(Key, Key)>>> {
        let fetched = self.0.fetch(
            FetchContext::default(),
            keys,
            FileAttributes::CONTENT_HEADER,
        );
        let iter = fetched
            .into_iter()
            .filter_map(|result| -> Option<anyhow::Result<(Key, Key)>> {
                (move || -> anyhow::Result<Option<(Key, Key)>> {
                    let (key, store_file) = result?;
                    Ok(store_file.copy_info()?.map(|copy_from| (key, copy_from)))
                })()
                .transpose()
            });
        Ok(Box::new(iter))
    }

    fn get_local_aux(&self, _path: &RepoPath, id: HgId) -> anyhow::Result<Option<FileAuxData>> {
        self.0.get_local_aux_direct(&id)
    }

    fn get_aux_iter(
        &self,
        fctx: FetchContext,
        keys: Vec<Key>,
    ) -> anyhow::Result<BoxIterator<anyhow::Result<(Key, FileAuxData)>>> {
        let fetched = self.0.fetch(fctx, keys, FileAttributes::AUX);
        let iter = fetched
            .into_iter()
            .map(|entry| -> anyhow::Result<(Key, FileAuxData)> {
                let (key, store_file) = entry?;
                let aux = store_file.aux_data()?;
                Ok((key, aux))
            });
        Ok(Box::new(iter))
    }

    fn clone_file_store(&self) -> Box<dyn storemodel::FileStore> {
        Box::new(self.clone())
    }
}

pub(crate) fn sha1_digest(opts: &InsertOpts, data: &[u8], format: SerializationFormat) -> Id20 {
    match format {
        SerializationFormat::Hg => {
            let p1 = opts.parents.first().copied().unwrap_or(NULL_ID);
            let p2 = opts.parents.get(1).copied().unwrap_or(NULL_ID);
            hg_sha1_digest(data, &p1, &p2)
        }
        SerializationFormat::Git => {
            let kind = match opts.kind {
                Kind::File => "blob",
                Kind::Tree => "tree",
            };
            git_sha1_digest(data, kind)
        }
    }
}
