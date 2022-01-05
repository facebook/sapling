/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Analyze tracing data for edenscm
//!
//! This is edenscm application specific. It's not a general purposed library.

mod tables;
pub use tables::extract_tables;
