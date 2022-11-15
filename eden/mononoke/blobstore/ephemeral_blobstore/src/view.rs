/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use changesets::Changesets;
use metaconfig_types::RepoConfig;
use repo_blobstore::RepoBlobstore;
use repo_identity::RepoIdentity;

#[facet::container]
pub struct EphemeralRepoView {
    #[facet]
    pub(crate) repo_blobstore: RepoBlobstore,

    #[facet]
    pub(crate) changesets: dyn Changesets,

    #[facet]
    pub(crate) repo_identity: RepoIdentity,

    #[facet]
    pub(crate) repo_config: RepoConfig,
}
