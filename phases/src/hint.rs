// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use Phase;
use blobrepo::BlobRepo;
use context::CoreContext;
use errors::*;
use futures::future;
use futures_ext::{BoxFuture, FutureExt};
use mononoke_types::ChangesetId;
use reachabilityindex::SkiplistIndex;

#[derive(Clone)]
pub struct PhasesHint {
    index: SkiplistIndex,
}

impl PhasesHint {
    pub fn new() -> Self {
        Self {
            index: SkiplistIndex::new(),
        }
    }

    /// Retrieve the phase specified by this commit, if available the commit exists
    /// Calculates it based on beeing ancestor of public bookmark.
    pub fn get(
        &self,
        _ctx: CoreContext,
        _repo: BlobRepo,
        _cs_id: ChangesetId,
    ) -> BoxFuture<Option<Phase>, Error> {
        // TODO (liubovd): implement calculation and cover with unit tests
        // currently everything is public
        future::ok(Some(Phase::Public)).boxify()
    }
}
