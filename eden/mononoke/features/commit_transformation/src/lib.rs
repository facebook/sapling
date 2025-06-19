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

// Re-exporting this module because there are many helper functions related to
// submodules that might be needed by the callers.
pub mod git_submodules;

pub use commit_rewriting::copy_file_contents;
pub use commit_rewriting::create_directory_source_to_target_multi_mover;
pub use commit_rewriting::create_source_to_target_multi_mover;
pub use commit_rewriting::rewrite_as_squashed_commit;
pub use commit_rewriting::rewrite_commit;
pub use commit_rewriting::rewrite_commit_with_file_changes_filter;
pub use commit_rewriting::rewrite_commit_with_implicit_deletes;
pub use commit_rewriting::upload_commits;
pub use implicit_deletes::get_renamed_implicit_deletes;
pub use types::CommitRewrittenToEmpty;
pub use types::DirectoryMultiMover;
pub use types::EmptyCommitFromLargeRepo;
pub use types::MultiMover;
pub use types::RewriteOpts;
pub use types::StripCommitExtras;
pub use types::SubmoduleDeps;
pub use types::SubmoduleExpansionContentIds;
pub use types::SubmodulePath;
