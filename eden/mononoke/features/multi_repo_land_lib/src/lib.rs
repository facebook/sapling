/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Stateless building blocks for multi-repo land flows, reusable without the
//! `multi_repo_land_service` binary's concrete `Repo` container.

mod manifest_commit;
mod repo_provider;
mod resolve;

pub use crate::manifest_commit::create_manifest_commit;
pub use crate::repo_provider::RepoProvider;
pub use crate::resolve::ResolveEntry;
pub use crate::resolve::ResolveOutcome;
pub use crate::resolve::ResolveResult;
pub use crate::resolve::resolve_bookmarks_cross_repo;
