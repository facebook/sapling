/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Stateless building blocks for multi-repo land flows, reusable without the
//! `multi_repo_land_service` binary's concrete `Repo` container.

mod manifest_commit;
mod repin;
mod repo_provider;
mod resolve;
mod scribe;

pub use crate::manifest_commit::create_manifest_commit;
pub use crate::repin::CasBaseline;
pub use crate::repin::ManifestCommitSpec;
pub use crate::repin::PreparedManifestCommit;
pub use crate::repin::RepinOptions;
pub use crate::repin::RepinOutcome;
pub use crate::repin::prepare_manifest_commit;
pub use crate::repin::repin_manifest_branch;
pub use crate::repo_provider::RepoProvider;
pub use crate::resolve::ResolveEntry;
pub use crate::resolve::ResolveOutcome;
pub use crate::resolve::ResolveResult;
pub use crate::resolve::bulk_read_bonsais_by_git_sha1;
pub use crate::resolve::bulk_read_bookmarks;
pub use crate::resolve::bulk_read_git_sha1s;
pub use crate::resolve::resolve_bookmarks_cross_repo;
pub use crate::scribe::log_scribe_bookmark_update;
