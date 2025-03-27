/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(trait_alias)]

// TODO(T182311609): stop exposing git_submodule directly in the public API
pub mod git_submodules;
pub(crate) mod rewrite;
mod types;

pub use rewrite::rewrite_commit;
pub use rewrite::SubmoduleExpansionContentIds;
pub use types::SubmoduleDeps;
pub use types::SubmodulePath;
