/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(async_closure)]
#![feature(trait_alias)]
#![feature(never_type)]

mod commit_in_memory_syncer;
mod commit_sync_config_utils;
mod commit_sync_outcome;
mod commit_syncer;
mod commit_syncers_lib;
mod git_submodules;
mod reporting;
mod sync_config_version_utils;
mod types;
mod validation;

pub use commit_sync_config_utils::get_bookmark_renamer;
pub use commit_sync_config_utils::get_common_pushrebase_bookmarks;
pub use commit_sync_config_utils::get_mover;
pub use commit_sync_config_utils::get_reverse_mover;
pub use commit_sync_config_utils::get_strip_git_submodules_by_version;
pub use commit_sync_config_utils::version_exists;
pub use commit_sync_outcome::commit_sync_outcome_exists;
pub use commit_sync_outcome::get_commit_sync_outcome;
pub use commit_sync_outcome::get_commit_sync_outcome_with_hint;
pub use commit_sync_outcome::get_plural_commit_sync_outcome;
pub use commit_sync_outcome::CandidateSelectionHint;
pub use commit_sync_outcome::CommitSyncOutcome;
pub use commit_sync_outcome::CommitSyncOutcome::*;
pub use commit_sync_outcome::PluralCommitSyncOutcome;
pub use commit_syncer::CommitSyncer;
pub use commit_syncers_lib::create_commit_syncer_lease;
pub use commit_syncers_lib::create_commit_syncers;
pub use commit_syncers_lib::find_toposorted_unsynced_ancestors;
pub use commit_syncers_lib::find_toposorted_unsynced_ancestors_with_commit_graph;
pub use commit_syncers_lib::get_version_and_parent_map_for_sync_via_pushrebase;
pub use commit_syncers_lib::get_x_repo_submodule_metadata_file_prefx_from_config;
pub use commit_syncers_lib::rewrite_commit;
pub use commit_syncers_lib::update_mapping_with_version;
pub use commit_syncers_lib::CommitSyncRepos;
pub use commit_syncers_lib::Syncers;
pub use git_submodules::SubmoduleExpansionData;
pub use reporting::CommitSyncContext;
pub use sync_config_version_utils::CHANGE_XREPO_MAPPING_EXTRA;
pub use types::ConcreteRepo;
pub use types::ErrorKind;
pub use types::Large;
pub use types::PushrebaseRewriteDates;
pub use types::Repo;
pub use types::Small;
pub use types::Source;
pub use types::SubmoduleDeps;
pub use types::Target;
pub use validation::find_bookmark_diff;
pub use validation::report_different;
pub use validation::verify_working_copy;
pub use validation::verify_working_copy_fast_path;
pub use validation::verify_working_copy_with_version_fast_path;
pub use validation::BookmarkDiff;
