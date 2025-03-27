/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod compact;
mod dummy_struct;
mod expand;
mod in_memory_repo;
mod sync;
pub mod utils;
mod validation;

pub use expand::SubmoduleExpansionData;
pub use in_memory_repo::InMemoryRepo;
pub use sync::sync_commit_with_submodule_expansion;
// TODO(T182311609): stop re-exporting the entire module
pub use utils::*;
pub use validation::validate_working_copy_of_expansion_with_recursive_submodules;

pub use crate::git_submodules::utils::RepoProvider;
pub use crate::git_submodules::validation::SubmoduleExpansionValidationToken;
pub use crate::git_submodules::validation::ValidSubmoduleExpansionBonsai;
