// (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

//! Internal Mercurial and Sapling configs.
//!
//! If you're modifying configs that apply to both internal and external [1],
//! update `../builtin_static/` instead.
//!
//! [1]: https://github.com/facebook/sapling

/// Modify this module for dynamic (conditional) system config.
pub(crate) mod dynamic_system;

/// Modify this module for static (unconditional) system config.
pub(crate) mod static_system;

/// Supporting libraries.
pub(crate) mod internalconfig;
pub(crate) mod thrift_types;

mod mode;

pub use mode::FbConfigMode;
