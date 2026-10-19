/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Perforce's data model, independent of how it is read.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Changelist {
    pub number: u64,
    pub user: Vec<u8>,
    /// Seconds since the epoch at which the changelist was submitted.
    pub time: u64,
    pub description: Vec<u8>,
}
