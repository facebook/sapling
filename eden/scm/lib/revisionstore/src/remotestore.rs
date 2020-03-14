/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{ops::Deref, sync::Arc};

use crate::{
    datastore::{HgIdMutableDeltaStore, RemoteDataStore},
    historystore::{HgIdMutableHistoryStore, RemoteHistoryStore},
};

pub trait HgIdRemoteStore: Send + Sync {
    fn datastore(&self, store: Arc<dyn HgIdMutableDeltaStore>) -> Arc<dyn RemoteDataStore>;
    fn historystore(&self, store: Arc<dyn HgIdMutableHistoryStore>) -> Arc<dyn RemoteHistoryStore>;
}

impl<T: HgIdRemoteStore + ?Sized, U: Deref<Target = T> + Send + Sync> HgIdRemoteStore for U {
    fn datastore(&self, store: Arc<dyn HgIdMutableDeltaStore>) -> Arc<dyn RemoteDataStore> {
        T::datastore(self, store)
    }

    fn historystore(&self, store: Arc<dyn HgIdMutableHistoryStore>) -> Arc<dyn RemoteHistoryStore> {
        T::historystore(self, store)
    }
}
