/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Logic to deal with time intervals: overlap, count, subtraction.

mod spanset;
mod time_interval;

pub use crate::time_interval::BlockedInterval;
pub use crate::time_interval::CowStr;
pub use crate::time_interval::TimeInterval;
