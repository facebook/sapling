/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use blobrepo::BlobRepo;
use context::CoreContext;
use derived_data::BonsaiDerived;
use failure_ext::{Error, Fail};
use futures::Future;
use futures_ext::{BoxFuture, FutureExt};
use mononoke_types::ChangesetId;

mod derive;
mod mapping;

pub use mapping::{RootUnodeManifestId, RootUnodeManifestMapping};

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "Invalid bonsai changeset: {}", _0)]
    InvalidBonsai(String),
}

pub fn derive_unodes(ctx: CoreContext, repo: BlobRepo, cs_id: ChangesetId) -> BoxFuture<(), Error> {
    let unodes_derived_mapping = RootUnodeManifestMapping::new(repo.get_blobstore());
    RootUnodeManifestId::derive(ctx, repo, unodes_derived_mapping, cs_id)
        .map(|_| ())
        .boxify()
}
