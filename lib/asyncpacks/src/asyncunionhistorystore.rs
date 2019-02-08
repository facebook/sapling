// Copyright 2019 Facebook, Inc.

use std::path::PathBuf;

use failure::Error;
use futures::future::poll_fn;
use tokio::prelude::*;
use tokio_threadpool::blocking;

use revisionstore::{unionhistorystore::UnionHistoryStore, HistoryPack};

use crate::asynchistorystore::AsyncHistoryStore;

pub type AsyncUnionHistoryStore<T> = AsyncHistoryStore<UnionHistoryStore<T>>;
pub struct AsyncUnionHistoryStoreBuilder {}

impl AsyncUnionHistoryStoreBuilder {
    pub fn new(
        packs: Vec<PathBuf>,
    ) -> impl Future<Item = AsyncUnionHistoryStore<HistoryPack>, Error = Error> + Send + 'static
    {
        poll_fn({
            move || {
                blocking(|| {
                    let mut store = UnionHistoryStore::new();

                    for pack in packs.iter() {
                        store.add(HistoryPack::new(&pack)?);
                    }

                    Ok(store)
                })
            }
        })
        .from_err()
        .and_then(|res| res)
        .map(move |unionstore| AsyncUnionHistoryStore::new(unionstore))
    }
}
