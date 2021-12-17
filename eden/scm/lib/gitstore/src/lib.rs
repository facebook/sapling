/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! # gitstore
//!
//! Git object store for various trait impls in EdenSCM.

mod gitstore;
mod trait_impls;

pub use crate::gitstore::GitStore;
pub use git2;
