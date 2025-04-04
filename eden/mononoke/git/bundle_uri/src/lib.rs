/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use fbinit::FacebookInit;
use metaconfig_types::GitBundleURIConfig;
#[cfg(fbcode_build)]
use metaconfig_types::UriGeneratorType;
use mononoke_types::RepositoryId;

#[cfg(fbcode_build)]
mod facebook;

mod sql;

#[cfg(fbcode_build)]
pub use cdn::CdnManifoldBundleUrlGenerator;
#[cfg(fbcode_build)]
pub use facebook::cdn;

pub use crate::sql::SqlGitBundleMetadataStorage;
pub use crate::sql::SqlGitBundleMetadataStorageBuilder;

#[async_trait]
pub trait GitBundleMetadataStorage {
    async fn get_newest_bundle_list_for_repo(
        &self,
        repo_id: RepositoryId,
    ) -> Result<Option<BundleList>>;
    async fn get_newest_bundle_lists(&self) -> Result<HashMap<RepositoryId, BundleList>>;
}

#[async_trait]
pub trait GitBundleUrlGenerator {
    async fn get_url_for_bundle_handle(&self, ttl: i64, handle: &str) -> Result<String>;
}

#[async_trait]
impl GitBundleUrlGenerator for LocalFSBUndleUriGenerator {
    async fn get_url_for_bundle_handle(&self, _ttl: i64, handle: &str) -> Result<String> {
        Ok(format!("file://{}", handle))
    }
}

#[derive(Clone)]
pub struct LocalFSBUndleUriGenerator {}

#[facet::facet]
#[async_trait]
/// Facet trait powering git's bundle-uri feature
pub trait GitBundleUri: Send + Sync {
    /// Gets the latest list of git bundles which together comprise the whole repo.
    /// There might be None.
    async fn get_latest_bundle_list(&self) -> Result<Option<BundleList>>;

    async fn get_url_for_bundle_handle(&self, ttl: i64, handle: &str) -> Result<String>;

    /// The repository for which the bundles are being tracked
    fn repo_id(&self) -> RepositoryId;
}

#[derive(Clone, Debug)]
pub struct Bundle {
    pub handle: String,
    pub fingerprint: String,
    pub in_bundle_list_order: u64,
}

#[derive(Clone, Debug)]
pub struct BundleList {
    pub bundle_list_num: u64,
    pub bundles: Vec<Bundle>,
}

pub struct BundleUri<U> {
    pub bundle_metadata_storage: SqlGitBundleMetadataStorage,
    pub bundle_url_generator: U,
    pub repo_id: RepositoryId,
}

impl<U: Send + Sync + Clone + GitBundleUrlGenerator + 'static> BundleUri<U> {
    pub async fn new(
        storage: SqlGitBundleMetadataStorage,
        bundle_url_generator: U,
        repo_id: RepositoryId,
    ) -> Result<Self>
    where
        U: GitBundleUrlGenerator + Clone + Send + Sync,
    {
        Ok(Self {
            bundle_metadata_storage: storage,
            bundle_url_generator,
            repo_id,
        })
    }
}

#[cfg(fbcode_build)]
pub fn bundle_uri_arc(
    fb: FacebookInit,
    storage: SqlGitBundleMetadataStorage,
    repo_id: RepositoryId,
    config: &GitBundleURIConfig,
) -> Arc<dyn GitBundleUri + Send + Sync + 'static> {
    match &config.uri_generator_type {
        UriGeneratorType::Cdn { bucket, api_key } => Arc::new(BundleUri {
            bundle_metadata_storage: storage,
            bundle_url_generator: CdnManifoldBundleUrlGenerator::new(
                fb,
                bucket.clone(),
                api_key.clone(),
            ),
            repo_id,
        }),
        UriGeneratorType::Manifold {
            bucket: _,
            api_key: _,
        } => unimplemented!("Not supported yet"),
        UriGeneratorType::LocalFS => Arc::new(BundleUri {
            bundle_metadata_storage: storage,
            bundle_url_generator: LocalFSBUndleUriGenerator {},
            repo_id,
        }),
    }
}

#[cfg(not(fbcode_build))]
pub fn bundle_uri_arc(
    _fb: FacebookInit,
    storage: SqlGitBundleMetadataStorage,
    repo_id: RepositoryId,
    _config: &GitBundleURIConfig,
) -> Arc<dyn GitBundleUri + Send + Sync + 'static> {
    Arc::new(BundleUri {
        bundle_metadata_storage: storage,
        bundle_url_generator: LocalFSBUndleUriGenerator {},
        repo_id,
    })
}

#[async_trait]
impl<U: Clone + Send + GitBundleUrlGenerator + Sync> GitBundleUri for BundleUri<U> {
    fn repo_id(&self) -> RepositoryId {
        self.repo_id
    }

    async fn get_latest_bundle_list(&self) -> Result<Option<BundleList>> {
        self.bundle_metadata_storage.get_latest_bundle_list().await
    }

    async fn get_url_for_bundle_handle(&self, ttl: i64, handle: &str) -> Result<String> {
        self.bundle_url_generator
            .get_url_for_bundle_handle(ttl, handle)
            .await
    }
}
