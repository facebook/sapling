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
use failure_ext::Error;
use futures_ext::BoxFuture;
use mononoke_types::ChangesetId;

mod derive;

pub fn derive_deleted_manifest(
    _ctx: CoreContext,
    _repo: BlobRepo,
    _cs_id: ChangesetId,
) -> BoxFuture<(), Error> {
    unimplemented!();
}
