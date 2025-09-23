/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Internal Mercurial and Sapling configs.
//!
//! If you're modifying configs that apply to both internal and external [1],
//! update `../builtin_static/` instead.
//!
//! [1]: https://github.com/facebook/sapling

/// Modify this module for dynamic (conditional) system config.
pub(crate) mod dynamic_system;
pub(crate) mod remote_config_snapshot;

/// Modify this module for static (unconditional) system config.
pub(crate) mod static_system;

#[cfg(fbcode_build)]
pub(crate) mod acl_evaluator;
/// Supporting libraries.
pub(crate) mod internalconfig;
pub(crate) mod internalconfigs;
pub(crate) mod thrift_types;

mod mode;

pub use internalconfig::Domain;
pub use mode::FbConfigMode;
