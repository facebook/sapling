/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::SystemTime;

use anyhow::Error;
use anyhow::Result;

/// Represents a file modification time in Mercurial, in seconds since the unix epoch.
#[derive(Clone, Copy, PartialEq)]
pub struct HgModifiedTime(u64);

impl From<u64> for HgModifiedTime {
    fn from(value: u64) -> Self {
        HgModifiedTime(value)
    }
}

impl From<u32> for HgModifiedTime {
    fn from(value: u32) -> Self {
        HgModifiedTime(value.into())
    }
}

// Mask used to make "crazy" mtimes operable. We basically take
// "mtime % 2**31-1". Note that 0x7FFFFFFF is in 2038 - not that far off. We may
// want to reconsider this. https://bz.mercurial-scm.org/show_bug.cgi?id=2608 is
// the original upstream introduction of this workaround.
const CRAZY_MTIME_MASK: i64 = 0x7FFFFFFF;

impl From<SystemTime> for HgModifiedTime {
    fn from(value: SystemTime) -> Self {
        let signed_epoch = match value.duration_since(SystemTime::UNIX_EPOCH) {
            Ok(d) => d.as_secs() as i64,
            // value is before UNIX_EPOCH
            Err(err) => -(err.duration().as_secs() as i64),
        };

        // Handle crazy mtimes by masking into reasonable range. This is what
        // dirstate.py does, so we may get some modicum of compatibility by
        // using the same approach.
        HgModifiedTime((signed_epoch & CRAZY_MTIME_MASK) as u64)
    }
}

impl TryFrom<i32> for HgModifiedTime {
    type Error = Error;
    fn try_from(value: i32) -> Result<Self> {
        Ok(HgModifiedTime(value.try_into()?))
    }
}
