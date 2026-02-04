/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Shared utilities for derived data benchmarks.
//!
//! This module provides common infrastructure used by multiple benchmarks:
//! - `BlobstoreCounters` for counting blobstore operations
//! - `CountingBlobstore` wrapper that counts GETs/PUTs
//! - `Repo` facet container for benchmark repositories
//! - Helper functions for creating repos and generating test data

use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreIsPresent;
use blobstore::OverwriteStatus;
use blobstore::PutBehaviour;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::Bookmarks;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use context::CoreContext;
use delayblob::DelayedBlobstore;
use delayblob::Normal;
use fbinit::FacebookInit;
use filestore::FilestoreConfig;
use memblob::Memblob;
use mononoke_types::BlobstoreBytes;
use rand::Rng;
use rand::distributions::Alphanumeric;
use repo_blobstore::RepoBlobstore;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;
use test_repo_factory::TestRepoFactory;

/// Production-realistic latency: 10ms GET, 15ms PUT (same-datacenter)
pub const GET_LATENCY_MS: f64 = 10.0;
pub const GET_LATENCY_STDDEV_MS: f64 = 3.0;
pub const PUT_LATENCY_MS: f64 = 15.0;
pub const PUT_LATENCY_STDDEV_MS: f64 = 5.0;

/// Counters for blobstore operations
#[derive(Debug, Default)]
pub struct BlobstoreCounters {
    pub gets: AtomicU64,
    pub puts: AtomicU64,
    pub is_presents: AtomicU64,
}

impl BlobstoreCounters {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&self) {
        self.gets.store(0, Ordering::SeqCst);
        self.puts.store(0, Ordering::SeqCst);
        self.is_presents.store(0, Ordering::SeqCst);
    }

    pub fn snapshot(&self) -> (u64, u64, u64) {
        (
            self.gets.load(Ordering::SeqCst),
            self.puts.load(Ordering::SeqCst),
            self.is_presents.load(Ordering::SeqCst),
        )
    }
}

/// A blobstore wrapper that counts operations
#[derive(Debug)]
pub struct CountingBlobstore<B> {
    inner: B,
    counters: Arc<BlobstoreCounters>,
}

impl<B: std::fmt::Display> std::fmt::Display for CountingBlobstore<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "CountingBlobstore<{}>", &self.inner)
    }
}

impl<B> CountingBlobstore<B> {
    pub fn new(inner: B, counters: Arc<BlobstoreCounters>) -> Self {
        Self { inner, counters }
    }
}

#[async_trait]
impl<B: Blobstore> Blobstore for CountingBlobstore<B> {
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        self.counters.gets.fetch_add(1, Ordering::SeqCst);
        self.inner.get(ctx, key).await
    }

    async fn is_present<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
        self.counters.is_presents.fetch_add(1, Ordering::SeqCst);
        self.inner.is_present(ctx, key).await
    }

    async fn unlink<'a>(&'a self, ctx: &'a CoreContext, key: &'a str) -> Result<()> {
        self.inner.unlink(ctx, key).await
    }

    async fn put_explicit<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
        put_behaviour: PutBehaviour,
    ) -> Result<OverwriteStatus> {
        self.counters.puts.fetch_add(1, Ordering::SeqCst);
        self.inner
            .put_explicit(ctx, key, value, put_behaviour)
            .await
    }

    async fn put_with_status<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<OverwriteStatus> {
        self.counters.puts.fetch_add(1, Ordering::SeqCst);
        self.inner.put_with_status(ctx, key, value).await
    }
}

/// Repository container with all facets needed for benchmarks
#[facet::container]
#[derive(Clone)]
pub struct Repo {
    #[facet]
    pub repo_identity: RepoIdentity,

    #[facet]
    pub repo_blobstore: RepoBlobstore,

    #[facet]
    pub repo_derived_data: RepoDerivedData,

    #[facet]
    pub bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    pub bookmarks: dyn Bookmarks,

    #[facet]
    pub commit_graph: CommitGraph,

    #[facet]
    pub commit_graph_writer: dyn CommitGraphWriter,

    #[facet]
    pub filestore_config: FilestoreConfig,
}

/// Create a repo with production-like configuration:
/// - Memblob for fast in-memory storage (simulates cachelib/memcache)
/// - Optional DelayedBlobstore wrapper for realistic I/O latency
/// - CountingBlobstore wrapper for operation counting
pub async fn create_repo(
    fb: FacebookInit,
    counters: Arc<BlobstoreCounters>,
    use_delay: bool,
) -> Result<Repo> {
    let base_blobstore: Arc<dyn Blobstore> = Arc::new(Memblob::new(PutBehaviour::IfAbsent));

    // Optionally wrap with DelayedBlobstore to simulate production network latency
    let blobstore: Arc<dyn Blobstore> = if use_delay {
        let get_dist = Normal::new(GET_LATENCY_MS / 1000.0, GET_LATENCY_STDDEV_MS / 1000.0)
            .context("Invalid GET latency distribution")?;
        let put_dist = Normal::new(PUT_LATENCY_MS / 1000.0, PUT_LATENCY_STDDEV_MS / 1000.0)
            .context("Invalid PUT latency distribution")?;
        Arc::new(CountingBlobstore::new(
            DelayedBlobstore::new(base_blobstore, get_dist, put_dist),
            counters,
        ))
    } else {
        Arc::new(CountingBlobstore::new(base_blobstore, counters))
    };

    let mut factory = TestRepoFactory::new(fb)?;
    factory.with_blobstore(blobstore);

    let repo: Repo = factory.build().await?;
    Ok(repo)
}

/// Generate a random filename of the specified length
pub fn gen_filename(rng: &mut impl Rng, len: usize) -> String {
    std::iter::repeat_with(|| rng.sample(Alphanumeric))
        .take(len)
        .map(char::from)
        .collect()
}
