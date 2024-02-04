/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub mod command_state;
mod merge_state;

pub use merge_state::ConflictState;
pub use merge_state::MergeDriverState;
pub use merge_state::MergeState;
pub use merge_state::UnsupportedMergeRecords;
