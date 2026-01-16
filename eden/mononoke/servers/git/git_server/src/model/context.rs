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
use metaconfig_parser::RepoConfigs;
use mononoke_api::Mononoke;
use mononoke_app::args::TLSArgs;
use mononoke_repos::MononokeRepos;
use repo_authorization::AuthorizationContext;
use repo_permission_checker::RepoPermissionCheckerRef;
use stats::prelude::*;

use super::GitMethodInfo;
use super::Pushvars;
use super::method::GitMethod;
use crate::GitRepos;
use crate::Repo;
use crate::errors::GitServerContextErrorKind;

define_stats! {
    prefix = "mononoke.git.server";
    repo_added_on_demand: timeseries(Rate, Sum),
    repo_added_on_demand_failed: timeseries(Rate, Sum),
}

#[derive(Clone)]
pub struct RepositoryRequestContext {
    pub ctx: CoreContext,
    pub repo: Arc<Repo>,
    pub mononoke_repos: Arc<MononokeRepos<Repo>>,
    pub repo_configs: Arc<RepoConfigs>,
    pub pushvars: Pushvars,
}

impl RepositoryRequestContext {
    pub async fn instantiate(
        state: &mut State,
        method_info: GitMethodInfo,
    ) -> Result<Self, GitServerContextErrorKind> {
        state.put(method_info.clone());
        let pushvars = state.take::<Pushvars>();
        let req_ctx = state.borrow_mut::<RequestContext>();
        let ctx = req_ctx.ctx.clone();
        let git_ctx = GitServerContext::borrow_from(state);
        git_ctx.request_context(ctx, method_info, pushvars).await
    }

    pub fn bundle_uri_trusted_only(&self) -> bool {
        self.repo
            .repo_config
            .as_ref()
            .git_configs
            .git_bundle_uri
            .as_ref()
            .is_none_or(|buri| buri.trusted_only)
    }
}

#[derive(Clone)]
pub struct GitServerContextInner {
    repos: GitRepos,
    enforce_auth: bool,
    // Upstream LFS server to fetch missing LFS objects from
    upstream_lfs_server: Option<String>,
    // Used for communicating with upstream LFS server
    tls_args: Option<TLSArgs>,
}

impl GitServerContextInner {
    pub fn new(
        repos: GitRepos,
        enforce_auth: bool,
        upstream_lfs_server: Option<String>,
        tls_args: Option<TLSArgs>,
    ) -> Self {
        Self {
            repos,
            enforce_auth,
            upstream_lfs_server,
            tls_args,
        }
    }
}

#[derive(Clone, StateData)]
pub struct GitServerContext {
    inner: Arc<RwLock<GitServerContextInner>>,
}

impl GitServerContext {
    pub fn new(
        repos: GitRepos,
        enforce_auth: bool,
        upstream_lfs_server: Option<String>,
        tls_args: Option<TLSArgs>,
    ) -> Self {
        let inner = Arc::new(RwLock::new(GitServerContextInner::new(
            repos,
            enforce_auth,
            upstream_lfs_server,
            tls_args,
        )));
        Self { inner }
    }

    pub async fn request_context(
        &self,
        ctx: CoreContext,
        method_info: GitMethodInfo,
        pushvars: Pushvars,
    ) -> Result<RepositoryRequestContext, GitServerContextErrorKind> {
        // First, try to get the repo from the already loaded repos
        let initial_lookup = {
            let inner = self
                .inner
                .read()
                .expect("poisoned lock in git server context");
            match inner.repos.get(&method_info.repo) {
                Some(repo) => Ok((
                    repo,
                    inner.repos.repo_mgr.repos().clone(),
                    inner.enforce_auth,
                    inner.repos.repo_configs(),
                )),
                None => {
                    // Check if the repo exists in the global configuration
                    let repo_exists_in_config = inner
                        .repos
                        .repo_mgr
                        .configs()
                        .repo_configs()
                        .repos
                        .contains_key(&method_info.repo);
                    if repo_exists_in_config {
                        // Repo exists but is not loaded, we'll need to add it
                        Err(Some(inner.repos.repo_mgr.clone()))
                    } else {
                        // Repo does not exist at all
                        Err(None)
                    }
                }
            }
        };

        let (repo, mononoke_repos, enforce_authorization, repo_configs) = match initial_lookup {
            Ok(result) => result,
            Err(Some(repo_mgr)) => {
                // Repo exists in config but not loaded
                // Check if dynamic repo loading is enabled via JustKnobs
                let dynamic_loading_enabled =
                    justknobs::eval("scm/mononoke:git_server_dynamic_repo_loading", None, None)
                        .unwrap_or(false);

                if !dynamic_loading_enabled {
                    // Dynamic loading is disabled, return the old behavior
                    return Err(GitServerContextErrorKind::RepositoryNotLoaded(
                        method_info.repo.to_string(),
                    ));
                }

                // Add it dynamically - this is done outside the lock since add_repo is async
                let repo_name = method_info.repo.clone();
                let repo = match repo_mgr.add_repo(&repo_name).await {
                    Ok(repo) => {
                        STATS::repo_added_on_demand.add_value(1);
                        repo
                    }
                    Err(e) => {
                        STATS::repo_added_on_demand_failed.add_value(1);
                        return Err(GitServerContextErrorKind::RepoSetupError {
                            repo_name,
                            error: e.to_string(),
                        });
                    }
                };
                let inner = self
                    .inner
                    .read()
                    .expect("poisoned lock in git server context");
                (
                    repo,
                    inner.repos.repo_mgr.repos().clone(),
                    inner.enforce_auth,
                    inner.repos.repo_configs(),
                )
            }
            Err(None) => {
                // Repo does not exist at all
                return Err(GitServerContextErrorKind::RepositoryDoesNotExist(
                    method_info.repo.to_string(),
                ));
            }
        };

        acl_check(&ctx, &repo, enforce_authorization, method_info.method).await?;
        Ok(RepositoryRequestContext {
            ctx,
            repo,
            mononoke_repos,
            repo_configs,
            pushvars,
        })
    }

    pub fn upstream_lfs_server(&self) -> Result<Option<String>> {
        let inner = self
            .inner
            .read()
            .expect("poisoned lock in git server context");
        Ok(inner.upstream_lfs_server.clone())
    }

    pub fn tls_args(&self) -> Result<Option<TLSArgs>> {
        let inner = self
            .inner
            .read()
            .expect("poisoned lock in git server context");
        Ok(inner.tls_args.clone())
    }

    pub fn repo_as_mononoke_api(&self) -> Result<Mononoke<mononoke_api::Repo>> {
        let inner = self
            .inner
            .read()
            .expect("poisoned lock in git server context");
        inner.repos.repo_mgr.make_mononoke_api()
    }
}

async fn acl_check(
    ctx: &CoreContext,
    repo: &impl RepoPermissionCheckerRef,
    enforce_authorization: bool,
    method: GitMethod,
) -> Result<(), GitServerContextErrorKind> {
    let authz = AuthorizationContext::new_non_draft(ctx);
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
