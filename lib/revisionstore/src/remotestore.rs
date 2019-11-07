/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{ops::Deref, sync::Arc};

use crate::{
    datastore::{MutableDeltaStore, RemoteDataStore},
    historystore::{MutableHistoryStore, RemoteHistoryStore},
};

pub trait RemoteStore: Send + Sync {
    fn datastore(&self, store: Box<dyn MutableDeltaStore>) -> Arc<dyn RemoteDataStore>;
    fn historystore(&self, store: Box<dyn MutableHistoryStore>) -> Arc<dyn RemoteHistoryStore>;
}

impl<T: RemoteStore + ?Sized, U: Deref<Target = T> + Send + Sync> RemoteStore for U {
    fn datastore(&self, store: Box<dyn MutableDeltaStore>) -> Arc<dyn RemoteDataStore> {
        T::datastore(self, store)
    }

    fn historystore(&self, store: Box<dyn MutableHistoryStore>) -> Arc<dyn RemoteHistoryStore> {
        T::historystore(self, store)
    }
}
