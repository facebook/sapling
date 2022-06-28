/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Encapsulation of data required to replay Mercurial bundles in the bookmark
//! update log.

use std::collections::HashMap;

use mercurial_types::HgChangesetId;
use mononoke_types::RawBundle2Id;
use mononoke_types::Timestamp;

pub struct BundleReplayData {
    pub bundle2_id: RawBundle2Id,
    pub timestamps: HashMap<HgChangesetId, Timestamp>,
}

impl BundleReplayData {
    pub fn new(bundle2_id: RawBundle2Id) -> Self {
        BundleReplayData {
            bundle2_id,
            timestamps: HashMap::new(),
        }
    }

    pub fn new_with_timestamps(
        bundle2_id: RawBundle2Id,
        timestamps: HashMap<HgChangesetId, Timestamp>,
    ) -> Self {
        BundleReplayData {
            bundle2_id,
            timestamps,
        }
    }
}
