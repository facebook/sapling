/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use blobstore::Blobstore;
use mononoke_types::RepositoryId;
use prefixblob::PrefixBlobstore;
use redactedblobstore::RedactedBlobstore;
use scuba_ext::ScubaSampleBuilder;
use std::collections::HashMap;
use std::sync::Arc;

/// RedactedBlobstore should be part of every blobstore since it is a layer
/// which adds security by preventing users to access sensitive content.

/// Making PrefixBlobstore part of every blobstore does two things:
/// 1. It ensures that the prefix applies first, which is important for shared caches like
///    memcache.
/// 2. It ensures that all possible blobrepos use a prefix.
pub type RepoBlobstore = RedactedBlobstore<PrefixBlobstore<Arc<dyn Blobstore>>>;

pub struct RepoBlobstoreArgs {
    blobstore: RepoBlobstore,
    repoid: RepositoryId,
}

impl RepoBlobstoreArgs {
    pub fn new<T: Blobstore + Clone>(
        blobstore: T,
        redacted_blobs: Option<HashMap<String, String>>,
        repoid: RepositoryId,
        scuba_builder: ScubaSampleBuilder,
    ) -> Self {
        let blobstore: Arc<dyn Blobstore> = Arc::new(blobstore);
        let blobstore = RedactedBlobstore::new(
            PrefixBlobstore::new(blobstore, repoid.prefix()),
            redacted_blobs,
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
        let (blobstore, redacted_blobs, scuba_builder) = blobstore.into_parts();
        let non_prefixed_blobstore = blobstore.into_inner();
        let new_inner_blobstore = wrapper(non_prefixed_blobstore);
        Self::new(new_inner_blobstore, redacted_blobs, repoid, scuba_builder)
    }

    pub fn into_blobrepo_parts(self) -> (RepoBlobstore, RepositoryId) {
        let Self { blobstore, repoid } = self;
        (blobstore, repoid)
    }
}
