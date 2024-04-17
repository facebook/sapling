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

pub use expand::expand_and_validate_all_git_submodule_file_changes;
pub use expand::SubmoduleExpansionData;
pub use in_memory_repo::InMemoryRepo;
pub use validation::validate_all_submodule_expansions;
