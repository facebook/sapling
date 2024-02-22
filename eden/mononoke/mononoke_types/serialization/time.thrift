/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! ------------
//! IMPORTANT!!!
//! ------------
//! Do not change the order of the fields! Changing the order of the fields
//! results in compatible but *not* identical serializations, so hashes will
//! change.
//! ------------
//! IMPORTANT!!!
//! ------------

/// A point in time in a particular timezone.
///
/// The timezone is defined as seconds after (west of) UTC, so UTC-8 is encoded
/// as 28800 and UTC+13 is encoded as -46800.
///
/// DateTime fields do not have a reasonable default value.  They must
/// always be required or optional.
struct DateTime {
  1: required i64 timestamp_secs;
  2: required i32 tz_offset_secs;
} (rust.exhaustive)

/// Timestamp without timezone information.
typedef i64 Timestamp (rust.newtype)
