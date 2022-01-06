/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(async_closure)]

pub mod types;

mod queue;
pub use queue::{AsyncMethodRequestQueue, ClaimedBy, RequestId};

pub mod tokens {
    pub use crate::types::{
        MegarepoAddBranchingTargetToken, MegarepoAddTargetToken, MegarepoChangeTargetConfigToken,
        MegarepoRemergeSourceToken, MegarepoSyncChangesetToken,
    };
}
