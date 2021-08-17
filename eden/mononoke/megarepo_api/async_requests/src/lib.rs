/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub mod types;

mod queue;
pub use queue::{AsyncMethodRequestQueue, ClaimedBy, RequestId};

pub mod tokens {
    pub use crate::types::{
        MegarepoAddBranchingTargetToken, MegarepoAddTargetToken, MegarepoChangeTargetConfigToken,
        MegarepoRemergeSourceToken, MegarepoSyncChangesetToken,
    };
}
