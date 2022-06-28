/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(async_closure)]

pub mod types;

mod queue;
pub use queue::AsyncMethodRequestQueue;
pub use queue::ClaimedBy;
pub use queue::RequestId;

pub mod tokens {
    pub use crate::types::MegarepoAddBranchingTargetToken;
    pub use crate::types::MegarepoAddTargetToken;
    pub use crate::types::MegarepoChangeTargetConfigToken;
    pub use crate::types::MegarepoRemergeSourceToken;
    pub use crate::types::MegarepoSyncChangesetToken;
}
