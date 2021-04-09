/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use crate::{
    datastore::{HgIdMutableDeltaStore, RemoteDataStore},
    historystore::{HgIdMutableHistoryStore, RemoteHistoryStore},
};

pub trait HgIdRemoteStore: Send + Sync {
    fn datastore(
        self: Arc<Self>,
        store: Arc<dyn HgIdMutableDeltaStore>,
    ) -> Arc<dyn RemoteDataStore>;
    fn historystore(
        self: Arc<Self>,
        store: Arc<dyn HgIdMutableHistoryStore>,
    ) -> Arc<dyn RemoteHistoryStore>;
}
