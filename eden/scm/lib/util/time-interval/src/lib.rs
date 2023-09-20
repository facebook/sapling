/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Logic to deal with time intervals: overlap, count, subtraction.

mod spanset;
mod time_interval;

pub use crate::time_interval::BlockedInterval;
pub use crate::time_interval::CowStr;
pub use crate::time_interval::TimeInterval;
