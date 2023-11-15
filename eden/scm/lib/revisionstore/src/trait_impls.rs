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
use hgstore::strip_hg_file_metadata;
use minibytes::Bytes;
use storemodel::BoxIterator;
use types::HgId;
use types::Key;
use types::RepoPath;

use crate::scmstore::fetch::FetchMode;
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
    ) -> anyhow::Result<BoxIterator<anyhow::Result<(Key, Bytes)>>> {
        let store = Arc::clone(&self.0);
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
                    strip_hg_file_metadata(&data.into()).map(|(d, _)| (key, d))
                }
                Ok(StoreResult::NotFound(k)) => Err(format_err!("{:?} not found in store", k)),
            }
        });
        Ok(Box::new(iter))
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
                        let (_data, maybe_copy_from) = strip_hg_file_metadata(&data.into())?;
                        Ok(maybe_copy_from.map(|copy_from| (key, copy_from)))
                    }
                    StoreResult::NotFound(k) => Err(format_err!("{:?} not found in store", k)),
                }
            })()
            .transpose()
        });
        Ok(Box::new(iter))
    }
}

impl storemodel::KeyStore for ArcFileStore {
    fn get_content_iter(
        &self,
        keys: Vec<Key>,
    ) -> anyhow::Result<BoxIterator<anyhow::Result<(Key, Bytes)>>> {
        let fetched = self.0.fetch(
            keys.into_iter(),
            FileAttributes::CONTENT,
            FetchMode::AllowRemote,
        );
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
}

impl storemodel::FileStore for ArcFileStore {
    fn get_rename_iter(
        &self,
        keys: Vec<Key>,
    ) -> anyhow::Result<BoxIterator<anyhow::Result<(Key, Key)>>> {
        let fetched = self.0.fetch(
            keys.into_iter(),
            FileAttributes::CONTENT,
            FetchMode::AllowRemote,
        );
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

    fn get_local_aux(
        &self,
        path: &RepoPath,
        id: HgId,
    ) -> anyhow::Result<Option<edenapi_types::FileAuxData>> {
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
    ) -> anyhow::Result<BoxIterator<anyhow::Result<(Key, FileAuxData)>>> {
        let fetched = self.0.fetch(
            keys.into_iter(),
            FileAttributes::AUX,
            FetchMode::AllowRemote,
        );
        let iter = fetched
            .into_iter()
            .map(|entry| -> anyhow::Result<(Key, FileAuxData)> {
                let (key, store_file) = entry?;
                let aux = store_file.aux_data()?;
                Ok((key, aux))
            });
        Ok(Box::new(iter))
    }
}

const PREFETCH_CHUNK_SIZE: usize = 1000;
