/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::{Path, PathBuf};

use anyhow::{Error, Result};
use futures::future::poll_fn;
use tokio::prelude::*;
use tokio_threadpool::blocking;

use revisionstore::{uniondatastore::UnionDataStore, DataPack, DataStore};

use crate::asyncdatastore::AsyncDataStore;

pub type AsyncUnionDataStore<T> = AsyncDataStore<UnionDataStore<T>>;

fn new_store<T: DataStore + Send + Sync + 'static>(
    packs: Vec<PathBuf>,
    builder: impl Fn(&Path) -> Result<T> + Send + 'static,
) -> impl Future<Item = AsyncUnionDataStore<T>, Error = Error> + Send + 'static {
    poll_fn({
        move || {
            blocking(|| {
                let mut store = UnionDataStore::new();

                for pack in packs.iter() {
                    store.add(builder(&pack)?);
                }

                Ok(store)
            })
        }
    })
    .from_err()
    .and_then(|res| res)
    .map(move |unionstore| AsyncUnionDataStore::new_(unionstore))
}

impl AsyncUnionDataStore<DataPack> {
    pub fn new(
        packs: Vec<PathBuf>,
    ) -> impl Future<Item = AsyncUnionDataStore<DataPack>, Error = Error> + Send + 'static {
        new_store(packs, DataPack::new)
    }
}
