/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use mononoke_repos::MononokeRepos;

/// Looks up a repo by name, decoupling shared land logic from the concrete
/// `MononokeRepos` collection so callers (e.g. tests) can substitute their own.
pub trait RepoProvider<R>: Send + Sync {
    fn get_by_name(&self, name: &str) -> Option<Arc<R>>;
}

/// Blanket impl so callers can pass `MononokeRepos` directly.
impl<R> RepoProvider<R> for MononokeRepos<R>
where
    R: Send + Sync,
{
    fn get_by_name(&self, name: &str) -> Option<Arc<R>> {
        MononokeRepos::get_by_name(self, name)
    }
}
