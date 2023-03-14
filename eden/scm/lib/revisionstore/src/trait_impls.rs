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
use async_runtime::block_on;
use async_trait::async_trait;
use futures::stream;
use futures::stream::BoxStream;
use futures::Stream;
use futures::StreamExt;
use hgstore::strip_metadata;
use minibytes::Bytes;
use storemodel::ReadFileContents;
use storemodel::RefreshableReadFileContents;
use tokio::runtime::Handle;
use types::Key;

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
impl<T> ReadFileContents for ArcRemoteDataStore<T>
where
    T: RemoteDataStore + 'static + ?Sized,
{
    type Error = anyhow::Error;

    async fn read_file_contents(&self, keys: Vec<Key>) -> BoxStream<Result<(Bytes, Key)>> {
        stream_data_from_remote_data_store(self.0.clone(), keys)
            .map(|result| match result {
                Ok((data, key, _copy_from)) => Ok((data, key)),
                Err(err) => Err(err),
            })
            .boxed()
    }

    fn read_rename_metadata(&self, keys: Vec<Key>) -> Result<Vec<(Key, Option<Key>)>> {
        let items = block_on(
            stream_data_from_remote_data_store(self.0.clone(), keys)
                .map(|result| match result {
                    Ok((_data, key, copy_from)) => Ok((key, copy_from)),
                    Err(err) => Err(err),
                })
                .collect::<Vec<_>>(),
        );
        items.into_iter().collect()
    }
}

#[async_trait]
impl ReadFileContents for ArcFileStore {
    type Error = anyhow::Error;

    async fn read_file_contents(&self, keys: Vec<Key>) -> BoxStream<Result<(Bytes, Key)>> {
        stream_data_from_scmstore(self.0.clone(), keys)
            .map(|result| match result {
                Ok((data, key, _copy_from)) => Ok((data, key)),
                Err(err) => Err(err),
            })
            .boxed()
    }

    fn read_rename_metadata(&self, keys: Vec<Key>) -> Result<Vec<(Key, Option<Key>)>> {
        let items = block_on(
            stream_data_from_scmstore(self.0.clone(), keys)
                .map(|result| match result {
                    Ok((_data, key, copy_from)) => Ok((key, copy_from)),
                    Err(err) => Err(err),
                })
                .collect::<Vec<_>>(),
        );
        items.into_iter().collect()
    }
}

impl RefreshableReadFileContents for ArcFileStore {
    fn refresh(&self) -> Result<()> {
        FileStore::refresh(&self.0)
    }
}

const PREFETCH_CHUNK_SIZE: usize = 1000;
const FETCH_PARALLELISM: usize = 20;

fn stream_data_from_remote_data_store<DS: RemoteDataStore + Clone + 'static>(
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
                                Ok(StoreResult::Found(data)) => strip_metadata(&data.into())
                                    .map(|(d, copy_from)| (d, key.clone(), copy_from)),
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
