/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! # virtual-tree
//!
//! Virtualized trees for testing.
//!
//! Goals:
//! - Random access to the N-th root tree.
//!   Do not build up trees from 1 to N (O(N)).
//! - Large state to make the test interesting, with limited space.
//!   Do not simply zip a repo that can be 100s of MBs or GBs.
//! - Looks like real-world.
//!   Diff between N-th and (N+1)-th root trees look reasonably small.
//!   Tree structure looks reasonably irregular.
//!
//! Currently, virtual-tree starts from a serialized list of trees
//! pre-generated from a real-world repo, then "stretch" the trees
//! to larger ones to satisfy needs.

pub mod stretch;
pub mod types;

#[cfg(test)]
pub(crate) mod tests;
