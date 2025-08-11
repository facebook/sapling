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
#[cfg(fbcode_build)]
#[allow(unused)]
use configo::ConfigoClient;
use context::CoreContext;
use futures::future::try_join_all;
use mononoke_api::MononokeError;
use permission_checker::AclProvider;
use repo_authorization::AuthorizationContext;
use source_control as thrift;

use crate::source_control_impl::SourceControlServiceImpl;

/// See https://www.internalfb.com/wiki/Configerator/Configerator_Get_Started/Configerator_Concepts_&_Terms/Configerator_Config_Name/
#[allow(unused)]
const QUICK_REPO_DEFINITIONS_CONFIG_NAME: &str = "scm/mononoke/repos/quick_repo_definitions";

async fn ensure_acls_allow_repo_creation(
    ctx: CoreContext,
    repos: &[thrift::RepoCreationRequest],
    acl_provider: &dyn AclProvider,
) -> Result<(), scs_errors::ServiceError> {
    let authz = AuthorizationContext::new(&ctx);
    try_join_all(
        repos
            .iter()
            .map(|repo| authz.require_repo_create(&ctx, &repo.repo_name, acl_provider)),
    )
    .await
    .map_err(Into::<MononokeError>::into)?;
    Ok(())
}

async fn update_repos_acls(
    _params: &thrift::CreateReposParams,
) -> Result<(), scs_errors::ServiceError> {
    // TODO (Pierre):
    // ## What:
    // Ensure all these repos have the necessary ACLs set-up.
    // We need it here as it needs to happen prior to both creating the repositories in Mononoke
    // and in Metagit.
    // Also, eventually, we will remove `create_repos_in_metagit` and we will need for the ACLs to
    // be updated in that case too.
    // It's probably OK to create them here and let `scmadmin` attempt to create them, which will
    // become a no-op. That will mean that `scmadmin` will still function both as stand-alone or
    // when called from here.
    //
    // ## How:
    // All the ACL logic currently lives in `scmadmin`: https://www.internalfb.com/code/fbsource/[9e94dda302f98eb670145fd69e62bf10520fe414]/fbcode/scm/scmadmin/commands/repo.py?lines=240
    // Use the `commitChangeSpecification` thrift endpoint [see code](https://www.internalfb.com/code/fbsource/[2627fc9342e5ce9f58e5dcccbc82f3ee35ca1ee8]/fbcode/infrasec/authorization/if/authorization.thrift?lines=1500)
    // See [example use](https://www.internalfb.com/code/fbsource/[207c62c9a0ec60970a06de4e5366fe92231cdfedf]/fbcode/icsp/rust/acl_fixer/src/update_acl.rs?lines=29)
    Ok(())
}

#[cfg(fbcode_build)]
async fn create_repos_in_mononoke(
    _params: &thrift::CreateReposParams,
) -> Result<(), scs_errors::ServiceError> {
    // TODO (Pierre):
    // ## What:
    // Create these repositories in Mononoke.
    //
    // ## How:
    // * a) Find the free range of repository ids by calling `get_max_id` on `GitSourceOfTruthConfig`
    //   See [code](https://www.internalfb.com/code/fbsource/[1855d0a75c2214080433754d4b70010d9a997594]/fbcode/eden/mononoke/git_source_of_truth/src/lib.rs?lines=46)
    // * b) Try to write the list of new repos (repo, id, state == "reserved") at max_id + 1, max_id + 2, ...
    // * c) This query can fail for 2 reasons:
    //   * Id is not unique <- This indicates contention. Go back to a)
    //   * Repo name is not unique <- Fail with a descritive Error
    // * d) At this stage, we have reserved the repo ids we need, so we're not in contention
    //   anymore. We have time to do slow things, such as configuring these repositories in
    //   configerator to add the to Mononoke.
    //   Use configo to make a diff adding these repositories to quick_repo_definitions using
    //   the parameters passed in to `create_repos`via thrift
    //   Note: for authoring a config change, the configo API is what I need:
    //   https://www.internalfb.com/intern/rustdoc/fbcode/common/rust/configo:configo/configo/index.html
    //   Create the mutation, validate it and land it.
    // * e) Repo creation was successful, overwrite "reserved" with "mononoke" for these
    //   repositories. At this point, the repos were created and empty, so it's OK for their Source
    //   Of Truth to be in Mononoke already, ahead of creating them in Metagit and allowing pushes.
    Ok(())
}

#[cfg(not(fbcode_build))]
async fn create_repos_in_mononoke(
    _params: &thrift::CreateReposParams,
) -> Result<(), scs_errors::ServiceError> {
    println!("No access to configo in oss build");
    Ok(())
}

async fn create_repos_in_metagit(
    params: thrift::CreateReposParams,
) -> Result<(), scs_errors::ServiceError> {
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
                command.arg("--oncall").arg(repo.oncall_name);
            }
        }
        let output = command
            .output()
            .map_err(Into::<Error>::into)
            .map_err(Into::<MononokeError>::into)?;
        if !output.status.success() {
            Err(scs_errors::internal_error(format!(
                "Failed to create repo: {}, stderr: {}, stdout: {}",
                &repo.repo_name,
                String::from_utf8_lossy(&output.stderr),
                String::from_utf8_lossy(&output.stdout),
            )))?
        }
    }
    Ok(())
}

impl SourceControlServiceImpl {
    pub(crate) async fn create_repos(
        &self,
        ctx: CoreContext,
        params: thrift::CreateReposParams,
    ) -> Result<thrift::CreateReposToken, scs_errors::ServiceError> {
        ensure_acls_allow_repo_creation(ctx, &params.repos, self.acl_provider.as_ref()).await?;
        update_repos_acls(&params).await?;
        create_repos_in_mononoke(&params).await?;
        create_repos_in_metagit(params).await?;

        Ok(thrift::CreateReposToken {
            ..Default::default()
        })
    }

    // This impl does nothing right now - but will do more in the near future.
    pub(crate) async fn create_repos_poll(
        &self,
        _ctx: CoreContext,
        _token: thrift::CreateReposToken,
    ) -> Result<thrift::CreateReposPollResponse, scs_errors::ServiceError> {
        Ok(thrift::CreateReposPollResponse {
            result: Some(Default::default()),
            ..Default::default()
        })
    }
}
