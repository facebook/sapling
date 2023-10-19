/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
