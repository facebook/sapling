/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub mod types;

mod queue;
pub use queue::AsyncMethodRequestQueue;

pub mod tokens {
    pub use crate::types::{
        MegarepoAddTargetToken, MegarepoChangeTargetConfigToken, MegarepoRemergeSourceToken,
        MegarepoSyncChangesetToken,
    };
}
