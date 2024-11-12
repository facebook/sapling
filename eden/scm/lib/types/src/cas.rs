/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt;

use crate::Blake3;

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct CasDigest {
    pub hash: Blake3,
    pub size: u64,
}

// CAS is agnostic to the type of the digest, however this is useful for logging
#[derive(Debug, Clone, Copy)]
pub enum CasDigestType {
    Tree,
    File,
    Mixed,
}

impl fmt::Display for CasDigestType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CasDigestType::Tree => write!(f, "tree"),
            CasDigestType::File => write!(f, "file"),
            CasDigestType::Mixed => write!(f, "digest"),
        }
    }
}

#[derive(Default, Debug)]
pub struct CasFetchedStats {
    pub total_bytes_zdb: u64,
    pub total_bytes_zgw: u64,
    pub total_bytes_manifold: u64,
    pub total_bytes_hedwig: u64,
    pub queries_zdb: u64,
    pub queries_zgw: u64,
    pub queries_manifold: u64,
    pub queries_hedwig: u64,
    pub hits_files_local_cache: u64,
    pub hits_bytes_local_cache: u64,
    pub misses_files_local_cache: u64,
    pub misses_bytes_local_cache: u64,
}

impl CasFetchedStats {
    pub fn add(&mut self, other: &CasFetchedStats) {
        self.total_bytes_zdb += other.total_bytes_zdb;
        self.total_bytes_zgw += other.total_bytes_zgw;
        self.total_bytes_manifold += other.total_bytes_manifold;
        self.total_bytes_hedwig += other.total_bytes_hedwig;
        self.queries_zdb += other.queries_zdb;
        self.queries_zgw += other.queries_zgw;
        self.queries_manifold += other.queries_manifold;
        self.queries_hedwig += other.queries_hedwig;
        self.hits_files_local_cache += other.hits_files_local_cache;
        self.hits_bytes_local_cache += other.hits_bytes_local_cache;
        self.misses_files_local_cache += other.misses_files_local_cache;
        self.misses_bytes_local_cache += other.misses_bytes_local_cache;
    }
}

#[derive(Debug, Clone, Copy)]
pub enum CasPrefetchOutcome {
    Prefetched(CasDigest),
    Missing(CasDigest),
}
