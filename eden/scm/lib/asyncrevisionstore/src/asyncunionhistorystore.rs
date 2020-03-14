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

use revisionstore::{unionhistorystore::UnionHgIdHistoryStore, HgIdHistoryStore, HistoryPack};

use crate::asynchistorystore::AsyncHgIdHistoryStore;

pub type AsyncUnionHgIdHistoryStore<T> = AsyncHgIdHistoryStore<UnionHgIdHistoryStore<T>>;

fn new_store<T: HgIdHistoryStore + Send + Sync + 'static>(
    packs: Vec<PathBuf>,
    builder: impl Fn(&Path) -> Result<T> + Send + 'static,
) -> impl Future<Item = AsyncUnionHgIdHistoryStore<T>, Error = Error> + Send + 'static {
    poll_fn({
        move || {
            blocking(|| {
                let mut store = UnionHgIdHistoryStore::new();

                for pack in packs.iter() {
                    store.add(builder(&pack)?);
                }

                Ok(store)
            })
        }
    })
    .from_err()
    .and_then(|res| res)
    .map(move |unionstore| AsyncUnionHgIdHistoryStore::new_(unionstore))
}

impl AsyncUnionHgIdHistoryStore<HistoryPack> {
    pub fn new(
        packs: Vec<PathBuf>,
    ) -> impl Future<Item = AsyncUnionHgIdHistoryStore<HistoryPack>, Error = Error> + Send + 'static
    {
        new_store(packs, |path| HistoryPack::new(&path))
    }
}
