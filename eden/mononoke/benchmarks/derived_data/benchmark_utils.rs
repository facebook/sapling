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

use anyhow::Context;
use anyhow::Result;
use blobstore::Blobstore;
use blobstore::PutBehaviour;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::Bookmarks;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
pub use counting_blob::BlobstoreCounters;
pub use counting_blob::BlobstoreCountersSnapshot;
pub use counting_blob::CountingBlobstore;
use delayblob::DelayedBlobstore;
use delayblob::Normal;
use fbinit::FacebookInit;
use filestore::FilestoreConfig;
use memblob::Memblob;
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
