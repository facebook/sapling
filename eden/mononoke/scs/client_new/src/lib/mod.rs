/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Library contating code shared between commands.

pub(crate) mod bookmark;
pub(crate) mod commit;
pub(crate) mod commit_id;
pub(crate) mod diff;
pub(crate) mod path_tree;

use chrono::DateTime;
use chrono::FixedOffset;
use chrono::TimeZone;
use source_control as thrift;

pub fn datetime(datetime: &thrift::DateTime) -> DateTime<FixedOffset> {
    FixedOffset::east(datetime.tz).timestamp(datetime.timestamp, 0)
}
