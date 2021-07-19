/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use blobrepo::BlobRepo;
use ephemeral_blobstore::RepoEphemeralBlobstore;
use segmented_changelog_types::SegmentedChangelog;
use skiplist::SkiplistIndex;

// Eventually everything inside Repo should really be here
// The fields of BlobRepo that are not used in e.g. LFS server should also be moved here
// Each binary will then be able to only build what they use of the "repo attributes".
#[facet::container]
#[derive(Clone)]
pub struct InnerRepo {
    #[delegate()]
    pub blob_repo: BlobRepo,

    #[facet]
    pub skiplist_index: SkiplistIndex,

    #[facet]
    pub segmented_changelog: dyn SegmentedChangelog,

    #[facet]
    pub ephemeral_blobstore: RepoEphemeralBlobstore,
}
