/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use blobrepo::BlobRepo;
use skiplist::SkiplistIndex;
use std::sync::Arc;

// Eventually everything inside Repo should really be here
// The fields of BlobRepo that are not used in e.g. LFS server should also be moved here
// Each binary will then be able to only build what they use of the "repo attributes".
#[facet::container]
pub struct InnerRepo {
    #[delegate()]
    pub blob_repo: Arc<BlobRepo>,

    #[facet]
    pub skiplist_index: SkiplistIndex,
}
