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
use gotham::state::FromState;
use gotham::state::State;
use gotham_derive::StateData;
use gotham_ext::middleware::request_context::RequestContext;
use repo_authorization::AuthorizationContext;
use repo_permission_checker::RepoPermissionCheckerRef;
use slog::Logger;

use super::method::GitMethod;
use super::GitMethodInfo;
use crate::errors::GitServerContextErrorKind;
use crate::GitRepos;
use crate::Repo;

#[derive(Clone)]
pub struct RepositoryRequestContext {
    pub ctx: CoreContext,
    pub repo: Arc<Repo>,
}

impl RepositoryRequestContext {
    pub async fn instantiate(
        state: &mut State,
        method_info: GitMethodInfo,
    ) -> Result<Self, GitServerContextErrorKind> {
        state.put(method_info.clone());
        let req_ctx = state.borrow_mut::<RequestContext>();
        let ctx = req_ctx.ctx.clone();
        let git_ctx = GitServerContext::borrow_from(state);
        git_ctx.request_context(ctx, method_info).await
    }

    pub fn _logger(&self) -> &Logger {
        self.ctx.logger()
    }
}

#[derive(Clone)]
pub struct GitServerContextInner {
    repos: GitRepos,
    enforce_auth: bool,
    _logger: Logger,
}

impl GitServerContextInner {
    pub fn new(repos: GitRepos, enforce_auth: bool, _logger: Logger) -> Self {
        Self {
            repos,
            enforce_auth,
            _logger,
        }
    }
}

#[derive(Clone, StateData)]
pub struct GitServerContext {
    inner: Arc<RwLock<GitServerContextInner>>,
}

impl GitServerContext {
    pub fn new(repos: GitRepos, enforce_auth: bool, _logger: Logger) -> Self {
        let inner = Arc::new(RwLock::new(GitServerContextInner::new(
            repos,
            enforce_auth,
            _logger,
        )));
        Self { inner }
    }

    pub async fn request_context(
        &self,
        ctx: CoreContext,
        method_info: GitMethodInfo,
    ) -> Result<RepositoryRequestContext, GitServerContextErrorKind> {
        let (repo, enforce_authorization) = {
            let inner = self
                .inner
                .read()
                .expect("poisoned lock in git server context");
            match inner.repos.get(&method_info.repo) {
                Some(repo) => (repo, inner.enforce_auth),
                None => {
                    return Err(GitServerContextErrorKind::RepositoryDoesNotExist(
                        method_info.repo.to_string(),
                    ));
                }
            }
        };
        acl_check(&ctx, &repo, enforce_authorization, method_info.method).await?;
        Ok(RepositoryRequestContext { ctx, repo })
    }
}

async fn acl_check(
    ctx: &CoreContext,
    repo: &impl RepoPermissionCheckerRef,
    enforce_authorization: bool,
    method: GitMethod,
) -> Result<(), GitServerContextErrorKind> {
    let authz = AuthorizationContext::new(ctx);
    let acl_check = if method.is_read_only() {
        authz.check_full_repo_read(ctx, repo).await
    } else {
        authz.check_full_repo_draft(ctx, repo).await
    };

    if acl_check.is_denied() && enforce_authorization {
        Err(GitServerContextErrorKind::Forbidden)
    } else {
        Ok(())
    }
}
