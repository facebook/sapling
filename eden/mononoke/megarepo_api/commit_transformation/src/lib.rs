/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(trait_alias)]

mod commit_rewriting;
mod implicit_deletes;
#[cfg(test)]
mod test;
mod types;

// TODO(T182311609): stop exposing git_submodule directly in the public API
pub mod git_submodules;

// TODO(T182311609): refine exports
pub use commit_rewriting::*;
pub use implicit_deletes::get_renamed_implicit_deletes;
pub use types::SubmoduleDeps;
pub use types::SubmoduleExpansionContentIds;
pub use types::SubmodulePath;
pub use types::*;
