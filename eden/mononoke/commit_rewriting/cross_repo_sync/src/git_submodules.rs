/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod dummy_struct;
mod expand;
mod in_memory_repo;
mod utils;
mod validation;

pub use expand::rewrite_commit_with_submodule_expansion;
pub use expand::SubmoduleExpansionData;
pub(crate) use expand::SubmodulePath;
pub use in_memory_repo::InMemoryRepo;
pub(crate) use utils::get_x_repo_submodule_metadata_file_path;
pub use validation::validate_all_submodule_expansions;
