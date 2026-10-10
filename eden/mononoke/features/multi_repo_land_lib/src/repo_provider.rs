/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use futures::FutureExt;
use futures::future::BoxFuture;
use mononoke_repos::MononokeRepos;

/// Looks up a repo by name, decoupling shared land logic from the concrete
/// `MononokeRepos` collection so callers (e.g. tests) can substitute their own.
pub trait RepoProvider<R: Send + Sync>: Send + Sync {
    fn get_by_name(&self, name: &str) -> Option<Arc<R>>;

    /// Like `get_by_name`, but a provider that can load a served repo on first
    /// use does so here; `None` still means the repo is not served at all.
    fn get_or_load<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Option<Arc<R>>>>
    where
        R: 'a,
    {
        std::future::ready(Ok(self.get_by_name(name))).boxed()
    }
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
