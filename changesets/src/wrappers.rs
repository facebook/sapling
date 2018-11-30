// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Implementations for wrappers that enable dynamic dispatch. Add more as necessary.

use std::sync::Arc;

use context::CoreContext;
use futures_ext::BoxFuture;
use mercurial_types::RepositoryId;
use mononoke_types::ChangesetId;

use {ChangesetEntry, ChangesetInsert, Changesets};
use errors::*;

impl Changesets for Arc<Changesets> {
    fn add(&self, ctx: CoreContext, cs: ChangesetInsert) -> BoxFuture<bool, Error> {
        (**self).add(ctx, cs)
    }

    fn get(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        cs_id: ChangesetId,
    ) -> BoxFuture<Option<ChangesetEntry>, Error> {
        (**self).get(ctx, repo_id, cs_id)
    }
}
