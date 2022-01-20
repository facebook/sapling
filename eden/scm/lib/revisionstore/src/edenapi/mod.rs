/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::marker::PhantomData;
use std::sync::Arc;

use async_trait::async_trait;
use edenapi::BlockingResponse;
use edenapi::EdenApi;
use edenapi::EdenApiError;
use edenapi::Response;
use edenapi_types::EdenApiServerError;
use edenapi_types::FileResponse;
use edenapi_types::FileSpec;
use edenapi_types::TreeAttributes;
use edenapi_types::TreeEntry;
use types::Key;

use crate::datastore::HgIdMutableDeltaStore;
use crate::datastore::RemoteDataStore;
use crate::historystore::HgIdMutableHistoryStore;
use crate::historystore::RemoteHistoryStore;
use crate::remotestore::HgIdRemoteStore;
use crate::types::StoreKey;

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
#[derive(Clone)]
pub struct EdenApiRemoteStore<T> {
    client: Arc<dyn EdenApi>,
    _phantom: PhantomData<T>,
}

impl<T: EdenApiStoreKind> EdenApiRemoteStore<T> {
    /// Create a new EdenApiRemoteStore using the given EdenAPI client.
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
    /// let store = EdenApiStore::<File>::new(edenapi);
    /// ```
    pub fn new(client: Arc<dyn EdenApi>) -> Arc<Self> {
        Arc::new(Self {
            client,
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

impl EdenApiFileStore {
    pub fn files_blocking(
        &self,
        keys: Vec<Key>,
    ) -> Result<BlockingResponse<FileResponse>, EdenApiError> {
        BlockingResponse::from_async(self.client.files(keys))
    }

    pub fn files_attrs_blocking(
        &self,
        reqs: Vec<FileSpec>,
    ) -> Result<BlockingResponse<FileResponse>, EdenApiError> {
        BlockingResponse::from_async(self.client.files_attrs(reqs))
    }

    pub async fn files_attrs(
        &self,
        reqs: Vec<FileSpec>,
    ) -> Result<Response<FileResponse>, EdenApiError> {
        self.client.files_attrs(reqs).await
    }
}

impl EdenApiTreeStore {
    pub fn trees_blocking(
        &self,
        keys: Vec<Key>,
        attributes: Option<TreeAttributes>,
    ) -> Result<BlockingResponse<Result<TreeEntry, EdenApiServerError>>, EdenApiError> {
        BlockingResponse::from_async(self.client.trees(keys, attributes))
    }
}

/// Trait that provides a common interface for calling the `files` and `trees`
/// methods on an EdenAPI client.
#[async_trait]
pub trait EdenApiStoreKind: Send + Sync + 'static {
    async fn prefetch_files(
        _client: Arc<dyn EdenApi>,
        _keys: Vec<Key>,
    ) -> Result<Response<FileResponse>, EdenApiError> {
        unimplemented!("fetching files not supported for this store")
    }

    async fn prefetch_trees(
        _client: Arc<dyn EdenApi>,
        _keys: Vec<Key>,
        _attributes: Option<TreeAttributes>,
    ) -> Result<Response<Result<TreeEntry, EdenApiServerError>>, EdenApiError> {
        unimplemented!("fetching trees not supported for this store")
    }
}

#[async_trait]
impl EdenApiStoreKind for File {
    async fn prefetch_files(
        client: Arc<dyn EdenApi>,
        keys: Vec<Key>,
    ) -> Result<Response<FileResponse>, EdenApiError> {
        client.files(keys).await
    }
}

#[async_trait]
impl EdenApiStoreKind for Tree {
    async fn prefetch_trees(
        client: Arc<dyn EdenApi>,
        keys: Vec<Key>,
        attributes: Option<TreeAttributes>,
    ) -> Result<Response<Result<TreeEntry, EdenApiServerError>>, EdenApiError> {
        client.trees(keys, attributes).await
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
