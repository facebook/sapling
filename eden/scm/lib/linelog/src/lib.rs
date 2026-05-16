/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

mod linelog;
mod maybe_mut;
mod small_revs;
mod stacks;

pub use crate::linelog::AbstractLineLog;
pub use crate::small_revs::SmallRevs;
pub use crate::stacks::FlattenLine;

/// LineLog with string line content.
pub type LineLog = AbstractLineLog<String>;

#[cfg(test)]
mod tests;
