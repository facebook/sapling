// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobstore::Blobstore;
use censoredblob::CensoredBlob;
use mononoke_types::RepositoryId;
use prefixblob::PrefixBlobstore;
use scuba_ext::ScubaSampleBuilder;
use std::collections::HashMap;
use std::sync::Arc;

/// CensoredBlob should be part of every blobstore since it is a layer
/// which adds security by preventing users to access sensitive content.

/// Making PrefixBlobstore part of every blobstore does two things:
/// 1. It ensures that the prefix applies first, which is important for shared caches like
///    memcache.
/// 2. It ensures that all possible blobrepos use a prefix.
pub type RepoBlobstore = CensoredBlob<PrefixBlobstore<Arc<dyn Blobstore>>>;

pub struct RepoBlobstoreArgs {
    blobstore: RepoBlobstore,
    repoid: RepositoryId,
}

impl RepoBlobstoreArgs {
    pub fn new<T: Blobstore + Clone>(
        blobstore: T,
        censored_blobs: Option<HashMap<String, String>>,
        repoid: RepositoryId,
        scuba_builder: ScubaSampleBuilder,
    ) -> Self {
        let blobstore: Arc<dyn Blobstore> = Arc::new(blobstore);
        let blobstore = CensoredBlob::new(
            PrefixBlobstore::new(blobstore, repoid.prefix()),
            censored_blobs,
            scuba_builder,
        );
        Self { blobstore, repoid }
    }

    pub fn new_with_wrapped_inner_blobstore<
        T: Blobstore + Clone,
        F: FnOnce(Arc<dyn Blobstore>) -> T,
    >(
        blobstore: RepoBlobstore,
        repoid: RepositoryId,
        wrapper: F,
    ) -> Self {
        let (blobstore, censored_blobs, scuba_builder) = blobstore.into_parts();
        let non_prefixed_blobstore = blobstore.into_inner();
        let new_inner_blobstore = wrapper(non_prefixed_blobstore);
        Self::new(new_inner_blobstore, censored_blobs, repoid, scuba_builder)
    }

    pub fn into_blobrepo_parts(self) -> (RepoBlobstore, RepositoryId) {
        let Self { blobstore, repoid } = self;
        (blobstore, repoid)
    }
}
