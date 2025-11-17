/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! # virtual-repo
//!
//! Virtualized repo data for testing.
//!
//! Implements:
//! - SHA1-like storage to access commits, trees, and files that are all
//!   generated on the fly, based on `virtual-tree`.
//! - Simple `dag` location<->hash lookups
//!
//! Integrates with:
//! - `EagerRepo`'s plug-in SHA1-like storage abstraction.
//!   `EagerRepo` implements a local storage abstraction and also a remote SLAPI
//!   abstraction (so the local storage can be `revisionstore`, closer to
//!   production environment). `virtual-repo` does not re-implement those
//!   features.

mod dag_populate;
mod dag_protocol;
mod eager_ext;
mod factory_impls;
mod file_size_gen;
mod id_fields;
mod provider;
pub mod text_gen;

pub use dag_populate::populate_dag;
pub use provider::VirtualRepoProvider;

pub fn init() {
    factory_impls::init();
}
