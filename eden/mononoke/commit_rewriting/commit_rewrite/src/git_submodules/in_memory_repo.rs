/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::fmt::Debug;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::BlobstoreBytes;
use blobstore::BlobstoreGetData;
use bonsai_hg_mapping::MemWritesBonsaiHgMapping;
use cacheblob::MemWritesBlobstore;
use context::CoreContext;
use derivative::Derivative;
use filestore::FilestoreConfig;
use futures::future;
use futures::stream;
use futures::StreamExt;
use readonlyblob::ReadOnlyBlobstore;
use repo_blobstore::RepoBlobstore;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;
use scuba_ext::MononokeScubaSampleBuilder;

use crate::git_submodules::dummy_struct::DummyStruct;
use crate::types::Repo;

/// Container to access a repo's blobstore and derived data without writing
/// anything to its blobstore or changeset table.
/// It's current purpose is to perform validation of git submodule expansion
/// by deriving fsnodes from uncommitted bonsais in the large repo.
#[facet::container]
#[derive(Clone)]
pub struct InMemoryRepo {
    #[facet]
    pub(crate) repo_blobstore: RepoBlobstore,

    #[facet]
    pub(crate) derived_data: RepoDerivedData,

    #[facet]
    filestore_config: FilestoreConfig,

    #[facet]
    repo_identity: RepoIdentity,
}

impl InMemoryRepo {
    pub fn from_repo<R: Repo + Send + Sync>(
        repo: &R,
        // Repos to fallback on blobstore reads if a blob isn't found on the
        // inner repo's blobstore
        fallback_repos: Vec<Arc<R>>,
    ) -> Result<Self> {
        let repo_identity = repo.repo_identity().clone();
        let original_blobstore = repo.repo_blobstore().clone();
        let repo_prefix = repo_identity.id().prefix();
        let filestore_config = repo.filestore_config().clone();

        let mem_writes_repo_blobstore =
            RepoBlobstore::new_with_wrapped_inner_blobstore(original_blobstore, |blobstore| {
                let readonly_blobstore = ReadOnlyBlobstore::new(blobstore);
                let mem_writes = Arc::new(MemWritesBlobstore::new(readonly_blobstore));

                Arc::new(MemWritesBlobstoreWithFallback::new(
                    mem_writes,
                    repo_prefix,
                    fallback_repos,
                ))
            });
        let memwrites_bonsai_hg_mapping =
            Arc::new(MemWritesBonsaiHgMapping::new(repo.bonsai_hg_mapping_arc()));
        let dummy_filenodes = Arc::new(DummyStruct);
        let dummy_bonsai_git_mapping = Arc::new(DummyStruct);

        let repo_derived_data = repo
            .repo_derived_data()
            .with_mutated_scuba(|_| MononokeScubaSampleBuilder::with_discard())
            .with_replaced_blobstore(mem_writes_repo_blobstore.clone())
            .with_replaced_bonsai_hg_mapping(memwrites_bonsai_hg_mapping)
            .with_replaced_filenodes(dummy_filenodes)
            .with_replaced_bonsai_git_mapping(dummy_bonsai_git_mapping)
            .with_replaced_derivation_service_client(None);

        Ok(Self {
            repo_blobstore: mem_writes_repo_blobstore.into(),
            derived_data: repo_derived_data.into(),
            filestore_config: filestore_config.into(),
            repo_identity: Arc::new(repo_identity.clone()),
        })
    }
}

#[derive(Clone, Derivative)]

struct MemWritesBlobstoreWithFallback<T, R: Repo + Clone> {
    inner: Arc<MemWritesBlobstore<T>>,
    /// Prefix of the inner repo's blobstore, required to update keys and
    /// read from the fallback repos' blobstores.
    inner_prefix: String,
    /// Repos to fallback on blobstore reads if a blob isn't found on the
    /// inner repo's blobstore
    fallback_blobstores: Vec<Arc<R>>,
}

impl<T: Blobstore + Clone, R: Repo + Clone> MemWritesBlobstoreWithFallback<T, R> {
    fn new(
        inner: Arc<MemWritesBlobstore<T>>,
        inner_prefix: String,
        fallback_blobstores: Vec<Arc<R>>,
    ) -> Self {
        Self {
            inner,
            inner_prefix,
            fallback_blobstores,
        }
    }

    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "MemWritesBlobstoreWithFallback<{0}, {1:#?}>",
            &self.inner,
            &self
                .fallback_blobstores
                .iter()
                .map(|repo| (
                    repo.repo_identity().name().to_string().clone(),
                    repo.repo_blobstore()
                ))
                .collect::<Vec<_>>()
        )
    }
}

#[async_trait]
impl<T: Blobstore + Clone, R: Repo + Clone> Blobstore for MemWritesBlobstoreWithFallback<T, R> {
    async fn put<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<()> {
        self.inner.put(ctx, key, value).await?;
        Ok(())
    }

    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        let mb_value = self.inner.get(ctx, key).await?;

        match mb_value {
            Some(value) => Ok(Some(value)),
            None => {
                // Query the fallback repos blobstores concurrently and return
                // the first `Ok(Some)` result found.
                let fallback_value = stream::iter(self.fallback_blobstores.clone())
                    .map(|repo| {
                        let blobstore: RepoBlobstore = repo.repo_blobstore().clone();
                        let new_key = key
                            .to_string()
                            .strip_prefix(&self.inner_prefix)
                            .unwrap()
                            .to_string();
                        async move {
                            let mb_val = blobstore.get(ctx, new_key.as_str()).await?;
                            anyhow::Ok(mb_val)
                        }
                    })
                    // Buffer all blobstore reads unordered
                    .buffer_unordered(10)
                    // Ignore all reads that don't return any result
                    .filter_map(|v| future::ready(v.transpose()))
                    // Get the first `Some` result
                    .next()
                    .await;
                fallback_value.transpose()
            }
        }
    }
}
impl<T: Blobstore + Clone, R: Repo + Clone> std::fmt::Display
    for MemWritesBlobstoreWithFallback<T, R>
{
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.fmt(f)
    }
}

impl<T: Blobstore + Clone, R: Repo + Clone> Debug for MemWritesBlobstoreWithFallback<T, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fmt(f)
    }
}
