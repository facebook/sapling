/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Implement traits defined by other crates.

use std::sync::Arc;

use anyhow::format_err;
use anyhow::Result;
use edenapi_types::FileAuxData;
use format_util::git_sha1_digest;
use format_util::hg_sha1_digest;
use format_util::strip_file_metadata;
use minibytes::Bytes;
use storemodel::BoxIterator;
use storemodel::InsertOpts;
use storemodel::KeyStore;
use storemodel::Kind;
use storemodel::SerializationFormat;
use types::fetch_mode::FetchMode;
use types::hgid::NULL_ID;
use types::HgId;
use types::Id20;
use types::Key;
use types::RepoPath;

use crate::scmstore::FileAttributes;
use crate::scmstore::FileStore;
use crate::RemoteDataStore;
use crate::StoreKey;
use crate::StoreResult;

// Wrapper types to workaround Rust's orphan rule.
#[derive(Clone)]
pub struct ArcFileStore(pub Arc<FileStore>);

pub struct ArcRemoteDataStore<T: ?Sized>(pub Arc<T>);

impl<T> storemodel::KeyStore for ArcRemoteDataStore<T>
where
    T: RemoteDataStore + 'static + ?Sized,
{
    fn get_content_iter(
        &self,
        keys: Vec<Key>,
        _fetch_mode: FetchMode,
    ) -> anyhow::Result<BoxIterator<anyhow::Result<(Key, Bytes)>>> {
        let store = Arc::clone(&self.0);
        let format = self.format();
        for chunk in keys.chunks(PREFETCH_CHUNK_SIZE) {
            let store_keys = chunk
                .iter()
                .map(|k| StoreKey::HgId(k.clone()))
                .collect::<Vec<_>>();
            store.prefetch(&store_keys)?;
        }
        let iter = keys.into_iter().map(move |key| {
            let store_result = store.get(StoreKey::HgId(key.clone()));
            match store_result {
                Err(err) => Err(err),
                Ok(StoreResult::Found(data)) => {
                    strip_file_metadata(&data.into(), format).map(|(d, _)| (key, d))
                }
                Ok(StoreResult::NotFound(k)) => Err(format_err!("{:?} not found in store", k)),
            }
        });
        Ok(Box::new(iter))
    }

    fn clone_key_store(&self) -> Box<dyn KeyStore> {
        Box::new(Self(self.0.clone()))
    }
}

impl<T> storemodel::FileStore for ArcRemoteDataStore<T>
where
    T: RemoteDataStore + 'static + ?Sized,
{
    fn get_rename_iter(
        &self,
        keys: Vec<Key>,
    ) -> anyhow::Result<BoxIterator<anyhow::Result<(Key, Key)>>> {
        let store = Arc::clone(&self.0);
        let format = self.format();
        for chunk in keys.chunks(PREFETCH_CHUNK_SIZE) {
            let store_keys = chunk
                .iter()
                .map(|k| StoreKey::HgId(k.clone()))
                .collect::<Vec<_>>();
            store.prefetch(&store_keys)?;
        }
        let iter = keys.into_iter().filter_map(move |key| {
            (|| {
                let store_result = store.get(StoreKey::HgId(key.clone()))?;
                match store_result {
                    StoreResult::Found(data) => {
                        let (_data, maybe_copy_from) = strip_file_metadata(&data.into(), format)?;
                        Ok(maybe_copy_from.map(|copy_from| (key, copy_from)))
                    }
                    StoreResult::NotFound(k) => Err(format_err!("{:?} not found in store", k)),
                }
            })()
            .transpose()
        });
        Ok(Box::new(iter))
    }

    fn clone_file_store(&self) -> Box<dyn storemodel::FileStore> {
        Box::new(Self(self.0.clone()))
    }
}

impl storemodel::KeyStore for ArcFileStore {
    fn get_content_iter(
        &self,
        keys: Vec<Key>,
        fetch_mode: FetchMode,
    ) -> anyhow::Result<BoxIterator<anyhow::Result<(Key, Bytes)>>> {
        let fetched = self.0.fetch(keys, FileAttributes::PURE_CONTENT, fetch_mode);
        let iter = fetched
            .into_iter()
            .map(|result| -> anyhow::Result<(Key, Bytes)> {
                let (key, mut store_file) = result?;
                let content = store_file.file_content()?;
                Ok((key, content))
            });
        Ok(Box::new(iter))
    }

    fn get_local_content(
        &self,
        _path: &RepoPath,
        hgid: HgId,
    ) -> anyhow::Result<Option<minibytes::Bytes>> {
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
        let fetched = self
            .0
            .fetch(keys, FileAttributes::CONTENT, FetchMode::AllowRemote);
        let iter = fetched
            .into_iter()
            .filter_map(|result| -> Option<anyhow::Result<(Key, Key)>> {
                (move || -> anyhow::Result<Option<(Key, Key)>> {
                    let (key, mut store_file) = result?;
                    let (_data, maybe_copy_from) = store_file.file_content_with_copy_info()?;
                    Ok(maybe_copy_from.map(|copy_from| (key, copy_from)))
                })()
                .transpose()
            });
        Ok(Box::new(iter))
    }

    fn get_local_aux(&self, path: &RepoPath, id: HgId) -> anyhow::Result<Option<FileAuxData>> {
        // PERF: This could be made faster by changes like D50935733.
        let key = Key::new(path.to_owned(), id);
        let fetched = self.0.fetch(
            std::iter::once(key),
            FileAttributes::AUX,
            FetchMode::LocalOnly,
        );
        if let Some(entry) = fetched.single()? {
            Ok(Some(entry.aux_data()?))
        } else {
            Ok(None)
        }
    }

    fn get_aux_iter(
        &self,
        keys: Vec<Key>,
        fetch_mode: FetchMode,
    ) -> anyhow::Result<BoxIterator<anyhow::Result<(Key, FileAuxData)>>> {
        let fetched = self.0.fetch(keys, FileAttributes::AUX, fetch_mode);
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

const PREFETCH_CHUNK_SIZE: usize = 1000;

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
