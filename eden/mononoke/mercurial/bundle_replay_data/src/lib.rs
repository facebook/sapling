/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

//! Encapsulation of data required to replay Mercurial bundles in the bookmark
//! update log.

use std::collections::HashMap;
use std::str::FromStr;

use anyhow::Result;
use bookmarks::{BundleReplay, RawBundleReplayData};
use mercurial_types::HgChangesetId;
use mononoke_types::{RawBundle2Id, Timestamp};

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

impl BundleReplay for BundleReplayData {
    fn to_raw(&self) -> Result<RawBundleReplayData> {
        Ok(RawBundleReplayData {
            bundle_handle: self.bundle2_id.to_hex().to_string(),
            commit_timestamps_json: serde_json::to_string(&self.timestamps)?,
        })
    }
}

impl TryFrom<&RawBundleReplayData> for BundleReplayData {
    type Error = anyhow::Error;

    fn try_from(raw: &RawBundleReplayData) -> Result<Self> {
        let bundle2_id = RawBundle2Id::from_str(&raw.bundle_handle)?;
        let timestamps = serde_json::from_str(&raw.commit_timestamps_json)?;

        Ok(BundleReplayData {
            bundle2_id,
            timestamps,
        })
    }
}

impl TryFrom<RawBundleReplayData> for BundleReplayData {
    type Error = anyhow::Error;

    fn try_from(raw: RawBundleReplayData) -> Result<Self> {
        BundleReplayData::try_from(&raw)
    }
}
