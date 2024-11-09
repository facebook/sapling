/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! # gitstore
//!
//! Git object store for various trait impls in Sapling.

mod factory_impls;
mod gitstore;
mod trait_impls;

pub use git2;

pub use crate::gitstore::GitStore;

/// Initialization. Register abstraction implementations.
pub fn init() {
    crate::factory_impls::setup_git_store_constructor();
}
