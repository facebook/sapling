/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Implement traits defined by other crates.

use std::sync::Arc;

use anyhow::anyhow;
use anyhow::format_err;
use anyhow::Result;
use async_trait::async_trait;
use futures::stream;
use futures::stream::BoxStream;
use futures::Stream;
use futures::StreamExt;
use hgstore::strip_hg_file_metadata;
use minibytes::Bytes;
use storemodel::BoxIterator;
use tokio::runtime::Handle;
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

#[async_trait]
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

#[async_trait]
impl<T> storemodel::FileStore for ArcRemoteDataStore<T>
where
    T: RemoteDataStore + 'static + ?Sized,
{
    async fn get_rename_stream(&self, keys: Vec<Key>) -> BoxStream<anyhow::Result<(Key, Key)>> {
        stream_data_from_remote_data_store(self.0.clone(), keys)
            .filter_map(|result| async move {
                match result {
                    Ok((_data, _key, None)) => None,
                    Ok((_data, key, Some(copy_from))) => Some(Ok((key, copy_from))),
                    Err(err) => Some(Err(err)),
                }
            })
            .boxed()
    }
}

#[async_trait]
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
        path: &RepoPath,
        hgid: HgId,
    ) -> anyhow::Result<Option<minibytes::Bytes>> {
        // PERF: unnecessary clones on path and key.
        let key = Key::new(path.to_owned(), hgid);
        self.0.get_file_content_impl(&key, FetchMode::LocalOnly)
    }

    fn refresh(&self) -> Result<()> {
        FileStore::refresh(&self.0)
    }
}

#[async_trait]
impl storemodel::FileStore for ArcFileStore {
    async fn get_rename_stream(&self, keys: Vec<Key>) -> BoxStream<anyhow::Result<(Key, Key)>> {
        stream_data_from_scmstore(self.0.clone(), keys)
            .filter_map(|result| async move {
                match result {
                    Ok((_data, _key, None)) => None,
                    Ok((_data, key, Some(copy_from))) => Some(Ok((key, copy_from))),
                    Err(err) => Some(Err(err)),
                }
            })
            .boxed()
    }
}

const PREFETCH_CHUNK_SIZE: usize = 1000;
const FETCH_PARALLELISM: usize = 20;

pub(crate) fn stream_data_from_remote_data_store<DS: RemoteDataStore + Clone + 'static>(
    store: DS,
    keys: Vec<Key>,
) -> impl Stream<Item = Result<(Bytes, Key, Option<Key>)>> {
    stream::iter(keys.into_iter().map(StoreKey::HgId))
        .chunks(PREFETCH_CHUNK_SIZE)
        .map(move |chunk| {
            let store = store.clone();
            Handle::current().spawn_blocking(move || {
                let mut data = vec![];
                match store.prefetch(&chunk) {
                    Err(e) => {
                        data.push(Err(e));
                    }
                    Ok(_) => {
                        for store_key in chunk.iter() {
                            let key = match store_key {
                                StoreKey::HgId(key) => key,
                                _ => unreachable!(),
                            };
                            let store_result = store.get(store_key.clone());
                            let result = match store_result {
                                Err(err) => Err(err),
                                Ok(StoreResult::Found(data)) => {
                                    strip_hg_file_metadata(&data.into())
                                        .map(|(d, copy_from)| (d, key.clone(), copy_from))
                                }
                                Ok(StoreResult::NotFound(k)) => {
                                    Err(format_err!("{:?} not found in store", k))
                                }
                            };
                            let is_err = result.is_err();
                            data.push(result);
                            if is_err {
                                break;
                            }
                        }
                    }
                };
                stream::iter(data.into_iter())
            })
        })
        .buffer_unordered(FETCH_PARALLELISM)
        .map(|r| {
            r.unwrap_or_else(|_| {
                stream::iter(vec![Err(anyhow!("background fetch join error"))].into_iter())
            })
        })
        .flatten()
}

fn stream_data_from_scmstore(
    store: Arc<FileStore>,
    keys: Vec<Key>,
) -> impl Stream<Item = Result<(Bytes, Key, Option<Key>)>> {
    stream::iter(keys.into_iter())
        .chunks(PREFETCH_CHUNK_SIZE)
        .map(move |chunk| {
            let store = store.clone();
            Handle::current().spawn_blocking(move || {
                let mut data = vec![];
                for result in store.fetch(
                    chunk.iter().cloned(),
                    FileAttributes::CONTENT,
                    FetchMode::AllowRemote,
                ) {
                    let result = match result {
                        Err(err) => Err(err.into()),
                        Ok((key, mut file)) => file
                            .file_content_with_copy_info()
                            .map(|(content, copy_from)| (content, key, copy_from)),
                    };
                    let is_err = result.is_err();
                    data.push(result);
                    if is_err {
                        break;
                    }
                }
                stream::iter(data.into_iter())
            })
        })
        .buffer_unordered(FETCH_PARALLELISM)
        .map(|r| {
            r.unwrap_or_else(|_| {
                stream::iter(vec![Err(anyhow!("background fetch join error"))].into_iter())
            })
        })
        .flatten()
}
