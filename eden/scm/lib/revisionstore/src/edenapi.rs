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
use edenapi::Response;
use edenapi::SaplingRemoteApi;
use edenapi::SaplingRemoteApiError;
use edenapi_types::FileResponse;
use edenapi_types::FileSpec;
use edenapi_types::SaplingRemoteApiServerError;
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

use data::SaplingRemoteApiDataStore;
use history::SaplingRemoteApiHistoryStore;

/// Convenience aliases for file and tree stores.
pub type SaplingRemoteApiFileStore = SaplingRemoteApiRemoteStore<File>;
pub type SaplingRemoteApiTreeStore = SaplingRemoteApiRemoteStore<Tree>;

/// A shim around an SaplingRemoteAPI client that implements the various traits of
/// Mercurial's storage layer, allowing a type that implements `SaplingRemoteApi` to be
/// used alongside other Mercurial data and history stores.
///
/// Note that this struct does not allow for data fetching on its own, because
/// it does not contain a mutable store into which to write the fetched data.
/// Use the methods from the `HgIdRemoteStore` trait to provide an appropriate
/// mutable store.
#[derive(Clone)]
pub struct SaplingRemoteApiRemoteStore<T> {
    client: Arc<dyn SaplingRemoteApi>,
    _phantom: PhantomData<T>,
}

impl<T: SaplingRemoteApiStoreKind> SaplingRemoteApiRemoteStore<T> {
    /// Create a new SaplingRemoteAPIRemoteStore using the given SaplingRemoteAPI client.
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
    /// let store = SaplingRemoteApiStore::<File>::new(edenapi);
    /// ```
    pub fn new(client: Arc<dyn SaplingRemoteApi>) -> Arc<Self> {
        Arc::new(Self {
            client,
            _phantom: PhantomData,
        })
    }

    /// Obtain the URL from the SaplingRemoteApi client.
    pub fn url(&self) -> Option<String> {
        self.client.url()
    }
}

impl HgIdRemoteStore for SaplingRemoteApiRemoteStore<File> {
    fn datastore(
        self: Arc<Self>,
        store: Arc<dyn HgIdMutableDeltaStore>,
    ) -> Arc<dyn RemoteDataStore> {
        Arc::new(SaplingRemoteApiDataStore::new(self, store))
    }

    fn historystore(
        self: Arc<Self>,
        store: Arc<dyn HgIdMutableHistoryStore>,
    ) -> Arc<dyn RemoteHistoryStore> {
        Arc::new(SaplingRemoteApiHistoryStore::new(self, store))
    }
}

impl HgIdRemoteStore for SaplingRemoteApiRemoteStore<Tree> {
    fn datastore(
        self: Arc<Self>,
        store: Arc<dyn HgIdMutableDeltaStore>,
    ) -> Arc<dyn RemoteDataStore> {
        Arc::new(SaplingRemoteApiDataStore::new(self, store))
    }

    fn historystore(
        self: Arc<Self>,
        _store: Arc<dyn HgIdMutableHistoryStore>,
    ) -> Arc<dyn RemoteHistoryStore> {
        unimplemented!("SaplingRemoteAPI does not support fetching tree history")
    }
}

/// Marker type indicating that the store fetches file data.
pub enum File {}

/// Marker type indicating that the store fetches tree data.
pub enum Tree {}

impl SaplingRemoteApiFileStore {
    pub fn files_blocking(
        &self,
        keys: Vec<Key>,
    ) -> Result<BlockingResponse<FileResponse>, SaplingRemoteApiError> {
        BlockingResponse::from_async(self.client.files(keys))
    }

    pub fn files_attrs_blocking(
        &self,
        reqs: Vec<FileSpec>,
    ) -> Result<BlockingResponse<FileResponse>, SaplingRemoteApiError> {
        BlockingResponse::from_async(self.client.files_attrs(reqs))
    }

    pub async fn files_attrs(
        &self,
        reqs: Vec<FileSpec>,
    ) -> Result<Response<FileResponse>, SaplingRemoteApiError> {
        self.client.files_attrs(reqs).await
    }
}

impl SaplingRemoteApiTreeStore {
    pub fn trees_blocking(
        &self,
        keys: Vec<Key>,
        attributes: Option<TreeAttributes>,
    ) -> Result<
        BlockingResponse<Result<TreeEntry, SaplingRemoteApiServerError>>,
        SaplingRemoteApiError,
    > {
        BlockingResponse::from_async(self.client.trees(keys, attributes))
    }
}

/// Trait that provides a common interface for calling the `files` and `trees`
/// methods on an SaplingRemoteAPI client.
#[async_trait]
pub trait SaplingRemoteApiStoreKind: Send + Sync + 'static {
    async fn prefetch_files(
        _client: Arc<dyn SaplingRemoteApi>,
        _keys: Vec<Key>,
    ) -> Result<Response<FileResponse>, SaplingRemoteApiError> {
        unimplemented!("fetching files not supported for this store")
    }

    async fn prefetch_trees(
        _client: Arc<dyn SaplingRemoteApi>,
        _keys: Vec<Key>,
        _attributes: Option<TreeAttributes>,
    ) -> Result<Response<Result<TreeEntry, SaplingRemoteApiServerError>>, SaplingRemoteApiError>
    {
        unimplemented!("fetching trees not supported for this store")
    }
}

#[async_trait]
impl SaplingRemoteApiStoreKind for File {
    async fn prefetch_files(
        client: Arc<dyn SaplingRemoteApi>,
        keys: Vec<Key>,
    ) -> Result<Response<FileResponse>, SaplingRemoteApiError> {
        client.files(keys).await
    }
}

#[async_trait]
impl SaplingRemoteApiStoreKind for Tree {
    async fn prefetch_trees(
        client: Arc<dyn SaplingRemoteApi>,
        keys: Vec<Key>,
        attributes: Option<TreeAttributes>,
    ) -> Result<Response<Result<TreeEntry, SaplingRemoteApiServerError>>, SaplingRemoteApiError>
    {
        client.trees(keys, attributes).await
    }
}

/// Return only the HgId keys from the given iterator.
/// SaplingRemoteAPI cannot fetch content-addressed LFS blobs.
fn hgid_keys<'a>(keys: impl IntoIterator<Item = &'a StoreKey>) -> Vec<Key> {
    keys.into_iter()
        .filter_map(|k| match k {
            StoreKey::HgId(k) => Some(k.clone()),
            StoreKey::Content(..) => None,
        })
        .collect()
}
