/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Types shared between the EdenAPI client and server.
//!
//! This crate exists primarily to provide a lightweight place to
//! put types that need to be used by both the client and server.
//! Types that are exclusive used by either the client or server
//! SHOULD NOT be added to this crate.
//!
//! Given that the client and server are each part of different
//! projects (Mercurial and Mononoke, respectively) which have
//! different build processes, putting shared types in their own
//! crate decreases the likelihood of build failures caused by
//! dependencies with complex or esoteric build requirements.
//!
//! Most of the types in this crate are used for data interchange
//! between the client and server. As such, CHANGES TO THE THESE
//! TYPES MAY CAUSE VERSION SKEW, so any changes should proceed
//! with caution.

#![deny(warnings)]

pub mod data;
pub mod history;
pub mod tree;

pub use crate::data::{DataEntry, DataRequest, DataResponse, Validity};
pub use crate::history::{
    HistoryEntry, HistoryRequest, HistoryResponse, HistoryResponseChunk, WireHistoryEntry,
};
pub use crate::tree::CompleteTreeRequest;
