/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use identity::Identity;
use land_service_if::LandChangesetRequest;
use mononoke_api::CoreContext;
use mononoke_api::Mononoke;

#[derive(Clone)]
pub struct LandChangesetObject {
    pub mononoke: Arc<Mononoke>,
    pub identity: Identity,
    pub ctx: CoreContext,
    pub request: LandChangesetRequest,
}

impl LandChangesetObject {
    pub fn new(
        mononoke: Arc<Mononoke>,
        identity: Identity,
        ctx: CoreContext,
        land_changesets: LandChangesetRequest,
    ) -> Self {
        Self {
            mononoke,
            identity,
            ctx,
            request: land_changesets,
        }
    }
}
