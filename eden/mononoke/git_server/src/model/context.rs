/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::sync::RwLock;

use anyhow::Result;
use context::CoreContext;
use gotham_derive::StateData;

use crate::errors::GitServerContextErrorKind;
use crate::GitRepos;
use crate::Repo;

#[derive(Clone)]
pub struct RepositoryRequestContext {
    pub ctx: CoreContext,
    pub repo: Arc<Repo>,
}

#[derive(Clone)]
pub struct GitServerContextInner {
    ctx: CoreContext,
    repos: GitRepos,
}

impl GitServerContextInner {
    pub fn new(ctx: CoreContext, repos: GitRepos) -> Self {
        Self { ctx, repos }
    }
}

#[derive(Clone, StateData)]
pub struct GitServerContext {
    inner: Arc<RwLock<GitServerContextInner>>,
}

#[allow(dead_code)]
impl GitServerContext {
    pub fn new(ctx: CoreContext, repos: GitRepos) -> Self {
        let inner = Arc::new(RwLock::new(GitServerContextInner::new(ctx, repos)));
        Self { inner }
    }

    pub fn request_context(
        &self,
        repo_name: &str,
    ) -> Result<RepositoryRequestContext, GitServerContextErrorKind> {
        let inner = self
            .inner
            .read()
            .expect("poisoned lock in git server context");
        let repo = inner.repos.get(repo_name).ok_or_else(|| {
            GitServerContextErrorKind::RepositoryDoesNotExist(repo_name.to_string())
        })?;
        Ok(RepositoryRequestContext {
            ctx: inner.ctx.clone(),
            repo,
        })
    }
}
