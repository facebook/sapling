// Copyright 2019 Facebook, Inc.

use std::path::PathBuf;

use failure::Error;
use futures::future::poll_fn;
use tokio::prelude::*;
use tokio_threadpool::blocking;

use revisionstore::{uniondatastore::UnionDataStore, DataPack};

use crate::asyncdatastore::AsyncDataStore;

pub type AsyncUnionDataStore<T> = AsyncDataStore<UnionDataStore<T>>;
pub struct AsyncUnionDataStoreBuilder {}

impl AsyncUnionDataStoreBuilder {
    pub fn new(
        packs: Vec<PathBuf>,
    ) -> impl Future<Item = AsyncUnionDataStore<DataPack>, Error = Error> + Send + 'static {
        poll_fn({
            move || {
                blocking(|| {
                    let mut store = UnionDataStore::new();

                    for pack in packs.iter() {
                        store.add(DataPack::new(&pack)?);
                    }

                    Ok(store)
                })
            }
        })
        .from_err()
        .and_then(|res| res)
        .map(move |unionstore| AsyncUnionDataStore::new(unionstore))
    }
}
