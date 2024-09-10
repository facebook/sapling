/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::env;
use std::process::Command;

use anyhow::Error;
use anyhow::Result;
use context::CoreContext;
use futures::future::try_join_all;
use mononoke_api::MononokeError;
use repo_authorization::AuthorizationContext;
use source_control as thrift;

use crate::errors;
use crate::source_control_impl::SourceControlServiceImpl;

impl SourceControlServiceImpl {
    pub(crate) async fn create_repos(
        &self,
        ctx: CoreContext,
        params: thrift::CreateReposParams,
    ) -> Result<thrift::CreateReposToken, errors::ServiceError> {
        let authz = AuthorizationContext::new(&ctx);
        try_join_all(params.repos.iter().map(|repo| {
            authz.require_repo_create(&ctx, &repo.repo_name, self.acl_provider.as_ref())
        }))
        .await
        .map_err(Into::<MononokeError>::into)?;

        let scmadmin_path = env::var("SCMADMIN_PATH")
            .map_err(Into::<Error>::into)
            .map_err(Into::<MononokeError>::into)?;

        for repo in params.repos {
            let mut command = Command::new(&scmadmin_path);

            command
                .arg("repo")
                .arg("init")
                .arg("--bypass-task-check")
                .arg("-r")
                .arg("git")
                .arg(&repo.repo_name);

            if params.dry_run {
                command.arg("--dry-run");
            }

            match repo.custom_acl {
                None => {
                    command.arg("--top-level-acl");
                }
                Some(custom_acl) => {
                    command.arg("--hipster-group").arg(custom_acl.hipster_group);
                }
            }
            let output = command
                .output()
                .map_err(Into::<Error>::into)
                .map_err(Into::<MononokeError>::into)?;
            if !output.status.success() {
                Err(errors::internal_error(format!(
                    "Failed to create repo: {}, stderr: {}, stdout: {}",
                    &repo.repo_name,
                    String::from_utf8_lossy(&output.stderr),
                    String::from_utf8_lossy(&output.stdout),
                )))?
            }
        }
        Ok(thrift::CreateReposToken {
            ..Default::default()
        })
    }

    // This impl does nothing right now - but will do more in the near future.
    pub(crate) async fn create_repos_poll(
        &self,
        _ctx: CoreContext,
        _token: thrift::CreateReposToken,
    ) -> Result<thrift::CreateReposPollResponse, errors::ServiceError> {
        Ok(thrift::CreateReposPollResponse {
            result: Some(Default::default()),
            ..Default::default()
        })
    }
}
