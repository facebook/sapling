/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use blobrepo::BlobRepo;
use bonsai_hg_mapping::BonsaiHgMapping;
use repo_blobstore::RepoBlobstore;

#[facet::container]
#[derive(Clone)]
/// The source repo for a given backup repo
pub struct BackupSourceRepo {
    #[facet]
    pub bonsai_hg_mapping: dyn BonsaiHgMapping,
    #[facet]
    pub repo_blobstore: RepoBlobstore,
}

impl BackupSourceRepo {
    pub fn from_blob_repo(repo: &BlobRepo) -> Self {
        Self {
            bonsai_hg_mapping: Arc::clone(repo.bonsai_hg_mapping()),
            repo_blobstore: Arc::new(repo.get_blobstore()),
        }
    }
}
