/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use blobrepo::BlobRepo;
use bookmarks::{BookmarkUpdateLog, Bookmarks};
use ephemeral_blobstore::RepoEphemeralStore;
use mutable_renames::MutableRenames;
use phases::Phases;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;
use segmented_changelog_types::SegmentedChangelog;
use skiplist::SkiplistIndex;

// Eventually everything inside Repo should really be here
// The fields of BlobRepo that are not used in e.g. LFS server should also be moved here
// Each binary will then be able to only build what they use of the "repo attributes".
#[facet::container]
#[derive(Clone)]
pub struct InnerRepo {
    #[delegate(dyn Bookmarks, dyn BookmarkUpdateLog, dyn Phases, RepoDerivedData, RepoIdentity)]
    pub blob_repo: BlobRepo,

    #[facet]
    pub skiplist_index: SkiplistIndex,

    #[facet]
    pub segmented_changelog: dyn SegmentedChangelog,

    #[facet]
    pub ephemeral_store: RepoEphemeralStore,

    #[facet]
    pub mutable_renames: MutableRenames,
}
