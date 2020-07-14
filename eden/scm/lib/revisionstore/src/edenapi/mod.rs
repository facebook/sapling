/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::marker::PhantomData;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use edenapi::{EdenApi, EdenApiError, Fetch, ProgressCallback, RepoName};
use edenapi_types::DataEntry;
use types::Key;

use crate::{
    datastore::{HgIdMutableDeltaStore, RemoteDataStore},
    historystore::{HgIdMutableHistoryStore, RemoteHistoryStore},
    remotestore::HgIdRemoteStore,
    types::StoreKey,
};

mod data;
mod history;

use data::EdenApiDataStore;
use history::EdenApiHistoryStore;

/// Convenience aliases for file and tree stores.
pub type EdenApiFileStore = EdenApiRemoteStore<File>;
pub type EdenApiTreeStore = EdenApiRemoteStore<Tree>;

/// A shim around an EdenAPI client that implements the various traits of
/// Mercurial's storage layer, allowing a type that implements `EdenApi` to be
/// used alongside other Mercurial data and history stores.
///
/// Note that this struct does not allow for data fetching on its own, because
/// it does not contain a mutable store into which to write the fetched data.
/// Use the methods from the `HgIdRemoteStore` trait to provide an appropriate
/// mutable store.
pub struct EdenApiRemoteStore<T> {
    client: Arc<dyn EdenApi>,
    repo: RepoName,
    _phantom: PhantomData<T>,
}

impl<T: EdenApiStoreKind> EdenApiRemoteStore<T> {
    /// Create a new EdenApiRemoteStore using the given EdenAPI client.
    ///
    /// In the current design of Mercurial's data storage layer, stores are
    /// typically tied to a particular repo. The `EdenApi` trait itself is
    /// repo-agnostic and requires the caller to specify the desired repo. As
    /// a result, an `EdenApiStore` needs to be passed the name of the repo
    /// it belongs to so it can pass it to the underlying EdenAPI client.edenapi
    ///
    /// The current design of the storage layer also requires a distinction
    /// between stores that provide file data and stores that provide tree data.
    /// (This is because both kinds of data are fetched via the `prefetch()`
    /// method from the `RemoteDataStore` trait.)
    ///
    /// The kind of data fetched by a store can be specified via a marker type;
    /// in particular, `File` or `Tree`. For example, a store that fetches file
    /// data would be created as follows:
    ///
    /// ```rust,ignore
    /// let store = EdenApiStore::<File>::new(repo, edenapi);
    /// ```
    pub fn new(repo: RepoName, client: Arc<dyn EdenApi>) -> Arc<Self> {
        Arc::new(Self {
            client,
            repo,
            _phantom: PhantomData,
        })
    }
}

impl HgIdRemoteStore for EdenApiRemoteStore<File> {
    fn datastore(
        self: Arc<Self>,
        store: Arc<dyn HgIdMutableDeltaStore>,
    ) -> Arc<dyn RemoteDataStore> {
        Arc::new(EdenApiDataStore::new(self, store))
    }

    fn historystore(
        self: Arc<Self>,
        store: Arc<dyn HgIdMutableHistoryStore>,
    ) -> Arc<dyn RemoteHistoryStore> {
        Arc::new(EdenApiHistoryStore::new(self, store))
    }
}

impl HgIdRemoteStore for EdenApiRemoteStore<Tree> {
    fn datastore(
        self: Arc<Self>,
        store: Arc<dyn HgIdMutableDeltaStore>,
    ) -> Arc<dyn RemoteDataStore> {
        Arc::new(EdenApiDataStore::new(self, store))
    }

    fn historystore(
        self: Arc<Self>,
        _store: Arc<dyn HgIdMutableHistoryStore>,
    ) -> Arc<dyn RemoteHistoryStore> {
        unimplemented!("EdenAPI does not support fetching tree history")
    }
}

/// Marker type indicating that the store fetches file data.
pub enum File {}

/// Marker type indicating that the store fetches tree data.
pub enum Tree {}

/// Trait that provides a common interface for calling the `files` and `trees`
/// methods on an EdenAPI client.
#[async_trait]
pub trait EdenApiStoreKind: Send + Sync + 'static {
    async fn prefetch(
        client: Arc<dyn EdenApi>,
        repo: RepoName,
        keys: Vec<Key>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<DataEntry>, EdenApiError>;
}

#[async_trait]
impl EdenApiStoreKind for File {
    async fn prefetch(
        client: Arc<dyn EdenApi>,
        repo: RepoName,
        keys: Vec<Key>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<DataEntry>, EdenApiError> {
        client.files(repo, keys, progress).await
    }
}

#[async_trait]
impl EdenApiStoreKind for Tree {
    async fn prefetch(
        client: Arc<dyn EdenApi>,
        repo: RepoName,
        keys: Vec<Key>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<DataEntry>, EdenApiError> {
        client.trees(repo, keys, progress).await
    }
}

/// Return only the HgId keys from the given iterator.
/// EdenAPI cannot fetch content-addressed LFS blobs.
fn hgid_keys<'a>(keys: impl IntoIterator<Item = &'a StoreKey>) -> Vec<Key> {
    keys.into_iter()
        .filter_map(|k| match k {
            StoreKey::HgId(k) => Some(k.clone()),
            StoreKey::Content(..) => None,
        })
        .collect()
}
