/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::Error;
use blobrepo::BlobRepo;
use context::CoreContext;
use derived_data::BonsaiDerived;
use futures::Future;
use futures_ext::{BoxFuture, FutureExt};
use mononoke_types::ChangesetId;

mod derive;
mod mapping;
mod ops;

pub use mapping::{RootDeletedManifestId, RootDeletedManifestMapping};
pub use ops::{find_entries, find_entry, list_all_entries};

pub fn derive_deleted_manifest(
    ctx: CoreContext,
    repo: BlobRepo,
    cs_id: ChangesetId,
) -> BoxFuture<(), Error> {
    RootDeletedManifestId::derive(ctx, repo, cs_id)
        .map(|_| ())
        .boxify()
}
