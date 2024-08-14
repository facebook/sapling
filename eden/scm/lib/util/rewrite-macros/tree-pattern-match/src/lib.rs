/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(test)]
mod tests;
mod tree_match;

pub use tree_match::find_all;
pub use tree_match::matches_full;
pub use tree_match::matches_start;
pub use tree_match::replace_all;
pub use tree_match::Item;
pub use tree_match::Match;
pub use tree_match::Placeholder;
pub use tree_match::PlaceholderExt;
pub use tree_match::Replace;
