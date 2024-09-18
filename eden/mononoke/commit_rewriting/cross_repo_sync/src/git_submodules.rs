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
mod utils;
mod validation;

pub use expand::SubmoduleExpansionData;
pub(crate) use expand::SubmodulePath;
pub use in_memory_repo::InMemoryRepo;
pub use sync::sync_commit_with_submodule_expansion;
pub(crate) use utils::build_recursive_submodule_deps;
pub use utils::get_all_repo_submodule_deps;
pub use utils::get_all_submodule_deps_from_repo_pair;
pub(crate) use utils::get_git_hash_from_submodule_file;
pub(crate) use utils::get_submodule_repo;
pub(crate) use utils::get_x_repo_submodule_metadata_file_path;
pub(crate) use utils::git_hash_from_submodule_metadata_file;
pub(crate) use utils::root_fsnode_id_from_submodule_git_commit;
pub(crate) use validation::validate_working_copy_of_expansion_with_recursive_submodules;

pub use crate::git_submodules::utils::RepoProvider;
pub use crate::git_submodules::validation::SubmoduleExpansionValidationToken;
pub use crate::git_submodules::validation::ValidSubmoduleExpansionBonsai;
