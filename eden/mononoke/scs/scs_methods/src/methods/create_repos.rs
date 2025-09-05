/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::env;
use std::process::Command;
use std::time::Duration;

use anyhow::Error;
use anyhow::Result;
use auth_consts::AUTH_SET;
use auth_consts::ONCALL;
use auth_consts::REPO;
use auth_consts::SANDCASTLE_CMD;
use auth_consts::SANDCASTLE_TAG;
use auth_consts::SERVICE_IDENTITY;
#[cfg(fbcode_build)]
#[allow(unused)]
use configo::ConfigoClient;
use context::CoreContext;
use futures::future::try_join_all;
use futures_retry::retry;
use git_source_of_truth::GitSourceOfTruth;
use git_source_of_truth::GitSourceOfTruthConfig;
use git_source_of_truth::RepositoryName;
use infrasec_authorization::ACL;
use infrasec_authorization::Identity;
use infrasec_authorization::consts as auth_consts;
use infrasec_authorization_service::CommitChangeSpecificationRequest;
use infrasec_authorization_service_srclients::make_AuthorizationService_srclient;
use infrasec_authorization_service_srclients::thrift::ChangeSpecification;
use infrasec_authorization_service_srclients::thrift::errors::AsNoConfigExistsException;
use mononoke_api::MononokeError;
use mononoke_api::RepositoryId;
use oncall::OncallClient;
use permission_checker::AclProvider;
use repo_authorization::AuthorizationContext;
use review_thrift::AclChange;
use review_thrift::AclMetadata;
use review_thrift::AclPermissionChange;
use review_thrift::ChangeContents;
use review_thrift::ChangeOperation;
use review_thrift::EntryChange;
use review_thrift::GroupChange;
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

#[cfg(fbcode_build)]
fn initial_acl_grants(hipster_group: &str) -> Vec<AclPermissionChange> {
    [
        (
            "read",
            vec![
                (AUTH_SET, "cocomatic_service_identities"),
                (AUTH_SET, "svcscm_read_all"),
                (AUTH_SET, "svnuser"),
                (SERVICE_IDENTITY, "aosp_megarepo_service_identity"),
                (SERVICE_IDENTITY, "gitremoteimport"),
                (SERVICE_IDENTITY, "scm_service_identity"),
                (SANDCASTLE_TAG, "skycastle_gitimport"),
                (SANDCASTLE_TAG, "skycastle_gitimport"),
                (SANDCASTLE_CMD, "SandcastleLandCommand"),
                (SANDCASTLE_CMD, "SandcastlePushCommand"),
            ],
        ),
        (
            "write",
            vec![
                (AUTH_SET, "svnuser"),
                (SANDCASTLE_CMD, "SandcastleLandCommand"),
                (SANDCASTLE_CMD, "SandcastlePushCommand"),
            ],
        ),
        (
            "bypass_readonly",
            vec![(AUTH_SET, "scm"), (SERVICE_IDENTITY, "gitremoteimport")],
        ),
        ("maintainers", vec![(AUTH_SET, hipster_group)]),
    ]
    .into_iter()
    .map(|(action, identities)| AclPermissionChange {
        action: action.to_string(),
        operation: ChangeOperation::UPDATE,
        entry_changes: identities
            .into_iter()
            .map(|(id_type, id_data)| EntryChange {
                entry: Identity {
                    id_type: id_type.to_string(),
                    id_data: id_data.to_string(),
                    ..Default::default()
                },
                operation: ChangeOperation::UPDATE,
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    })
    .collect()
}

#[cfg(fbcode_build)]
fn make_initial_acl_creation_request(
    acl_name: &str,
    oncall_name: &str,
    hipster_group: &str,
) -> CommitChangeSpecificationRequest {
    let grants = initial_acl_grants(hipster_group);
    let repo_group = Identity {
        id_type: REPO.to_string(),
        id_data: acl_name.to_string(),
        ..Default::default()
    };
    let acl_change = AclChange {
        acl: repo_group,
        permission_changes: grants,
        operation: ChangeOperation::UPDATE,
        metadata_update: Some(AclMetadata {
            oncall: Some(oncall_name.to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let group_change = GroupChange {
        change_data: acl_change,
        ..Default::default()
    };
    let change_contents = ChangeContents {
        group_changes: vec![group_change],
        ..Default::default()
    };
    let spec = ChangeSpecification::contents(change_contents);
    let reason = "automated repo creation".to_string();
    CommitChangeSpecificationRequest {
        spec,
        commit_message: reason,
        ..Default::default()
    }
}

#[cfg(fbcode_build)]
async fn create_repo_acl(
    ctx: CoreContext,
    acl_name: &str,
    oncall_name: &str,
    hipster_group: &str,
) -> Result<(), scs_errors::ServiceError> {
    let request = make_initial_acl_creation_request(acl_name, oncall_name, hipster_group);
    let thrift_client = make_AuthorizationService_srclient!(ctx.fb)
        .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?;
    thrift_client
        .commitChangeSpecification(&request)
        .await
        .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?;
    Ok(())
}

#[cfg(fbcode_build)]
async fn is_valid_oncall_name(
    ctx: CoreContext,
    oncall_name: &str,
    valid_oncall_names_cache: &mut HashSet<String>,
) -> Result<bool, scs_errors::ServiceError> {
    // Check the cache first
    if valid_oncall_names_cache.contains(oncall_name) {
        Ok(true)
    } else {
        // Sanity check the syntax to avoid making an unnecessary call to the oncall service
        if oncall_name
            .chars()
            .any(|c| !(c.is_ascii_digit() || c.is_ascii_lowercase() || c == '_'))
        {
            return Ok(false);
        }
        // Validate the oncall actually exists
        match OncallClient::new(ctx.fb)
            .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?
            .get_current_oncall(oncall_name)
            .await
        {
            Ok(_) => {
                // Cache the successful result to avoid unnecessary calls in the future
                valid_oncall_names_cache.insert(oncall_name.to_string());
                Ok(true)
            }
            Err(_) => Ok(false),
        }
    }
}

#[cfg(fbcode_build)]
async fn is_valid_hipster_group(
    ctx: CoreContext,
    hipster_group: &str,
    valid_hipster_groups_cache: &mut HashSet<String>,
) -> Result<bool, scs_errors::ServiceError> {
    // Check the cache first
    if valid_hipster_groups_cache.contains(hipster_group) {
        Ok(true)
    } else {
        // Sanity check the syntax to avoid making an unnecessary call to the oncall service
        if hipster_group
            .chars()
            .any(|c| !(c.is_ascii_digit() || c.is_ascii_lowercase() || c == '_'))
        {
            return Ok(false);
        }
        // Validate the hipster group actually exists
        let thrift_client = make_AuthorizationService_srclient!(ctx.fb)
            .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?;
        let hipster_group_identity = Identity {
            id_type: AUTH_SET.to_string(),
            id_data: hipster_group.to_string(),
            ..Default::default()
        };
        let exists = thrift_client
            .aclExists(&hipster_group_identity)
            .await
            .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?;
        if exists {
            // Cache the successful result to avoid unnecessary calls in the future
            valid_hipster_groups_cache.insert(hipster_group.to_string());
        }
        Ok(exists)
    }
}

#[cfg(fbcode_build)]
async fn validate_repo_acl(
    acl: ACL,
    acl_name: &str,
    oncall_name: &str,
    hipster_group: &str,
) -> Result<(), scs_errors::ServiceError> {
    // Ensure this hipster group (as AUTH_SET or ONCALL_GROUP type) is a maintainer for this ACL
    if !acl.permissions.iter().any(|permission| {
        permission.action == "maintainers"
            && permission.entries.iter().any(|entry| {
                entry.identity.id_data == hipster_group
                    && (entry.identity.id_type == AUTH_SET || entry.identity.id_type == ONCALL)
            })
    }) {
        return Err(scs_errors::invalid_request(format!(
            "Hipster group: {} is not a maintainer for acl: {}",
            hipster_group, acl_name
        ))
        .into());
    }
    // Ensure this oncall is point of contact for this ACL
    if acl.point_of_contact.id_data != oncall_name {
        return Err(scs_errors::invalid_request(format!(
            "Oncall: {} is not a point of contact for acl: {}",
            oncall_name, acl_name
        ))
        .into());
    }
    Ok(())
}

#[cfg(fbcode_build)]
async fn try_fetching_repo_acl(
    ctx: CoreContext,
    acl_name: &str,
) -> Result<Option<ACL>, scs_errors::ServiceError> {
    let thrift_client = make_AuthorizationService_srclient!(ctx.fb)
        .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?;

    let repo_group = Identity {
        id_type: REPO.to_string(),
        id_data: acl_name.to_string(),
        ..Default::default()
    };
    match thrift_client.getAuthConfig(&repo_group).await {
        Err(e) => {
            if e.as_no_config_exists_exception().is_some() {
                Ok(None)
            } else {
                Err(scs_errors::internal_error(format!("{e:#}")).into())
            }
        }
        Ok(auth_config) => Ok(Some(auth_config.acl)),
    }
}

#[cfg(fbcode_build)]
fn make_full_acl_name_from_repo_name(repo_name: &str) -> String {
    format!("REPOS:repos/git/{}", repo_name)
}

#[cfg(fbcode_build)]
fn make_top_level_acl_name_from_repo_name(repo_name: &str) -> String {
    let (top_level, _rest) = repo_name.split_once('/').unwrap_or((repo_name, ""));
    format!("REPOS:repos/git/{}", top_level)
}

/// Ensure all repos have the necessary ACLs set-up.
/// Note: Currently, this duplicates the logic that happens when creating the repositories in
/// Metagit by forking to `scmadmin` in `create_repos_in_metagit`, but this will go away
/// eventually, so we need this to happen here to prepare for that
#[cfg(fbcode_build)]
async fn update_repos_acls(
    ctx: CoreContext,
    params: &thrift::CreateReposParams,
) -> Result<(), scs_errors::ServiceError> {
    let mut valid_oncall_names_cache = HashSet::new();
    let mut valid_hipster_groups_cache = HashSet::new();
    for repo_creation_request in &params.repos {
        // If a custom acl is provided, we need to ensure it exists and is valid or create it
        if let Some(custom_acl) = &repo_creation_request.custom_acl {
            let acl_name = make_full_acl_name_from_repo_name(&repo_creation_request.repo_name);
            if !is_valid_oncall_name(
                ctx.clone(),
                &repo_creation_request.oncall_name,
                &mut valid_oncall_names_cache,
            )
            .await?
            {
                return Err(scs_errors::invalid_request(format!(
                    "Invalid oncall name: {}",
                    repo_creation_request.oncall_name
                ))
                .into());
            };
            if !is_valid_hipster_group(
                ctx.clone(),
                &custom_acl.hipster_group,
                &mut valid_hipster_groups_cache,
            )
            .await?
            {
                return Err(scs_errors::invalid_request(format!(
                    "Invalid hipster group: {}",
                    custom_acl.hipster_group
                ))
                .into());
            }
            if let Some(acl) = try_fetching_repo_acl(ctx.clone(), &acl_name).await? {
                validate_repo_acl(
                    acl,
                    &acl_name,
                    &repo_creation_request.oncall_name,
                    &custom_acl.hipster_group,
                )
                .await?;
            } else if !params.dry_run {
                create_repo_acl(
                    ctx.clone(),
                    &acl_name,
                    &repo_creation_request.oncall_name,
                    &custom_acl.hipster_group,
                )
                .await?;
            }
        }
        // If no custom acl is provided, we need to ensure the top level acl exists
        else {
            let acl_name = make_top_level_acl_name_from_repo_name(&repo_creation_request.repo_name);
            if try_fetching_repo_acl(ctx.clone(), &acl_name)
                .await?
                .is_none()
            {
                return Err(scs_errors::invalid_request(format!(
                    "Top level acl {acl_name} does not exist!"
                ))
                .into());
            }
        }
    }
    Ok(())
}

#[cfg(not(fbcode_build))]
async fn update_repos_acls(
    ctx: CoreContext,
    params: &thrift::CreateReposParams,
) -> Result<(), scs_errors::ServiceError> {
    println!("No access to hipster in oss build");
    Ok(())
}

#[cfg(fbcode_build)]
async fn reserve_repos_ids(
    ctx: CoreContext,
    git_source_of_truth_config: &dyn GitSourceOfTruthConfig,
    params: &thrift::CreateReposParams,
) -> Result<Vec<(RepositoryId, RepositoryName)>, scs_errors::ServiceError> {
    let max_id = git_source_of_truth_config
        .get_max_id(&ctx)
        .await
        .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?;
    if let Some(max_id) = max_id {
        let mut repo_id = max_id.id();
        let repos = params
            .repos
            .iter()
            .map(|request| {
                repo_id += 1;
                (
                    RepositoryId::new(repo_id),
                    RepositoryName(request.repo_name.clone()),
                    GitSourceOfTruth::Reserved,
                )
            })
            .collect::<Vec<_>>();
        let result = git_source_of_truth_config.insert_repos(&ctx, &repos).await;
        match result {
            Ok(_) => Ok(repos
                .into_iter()
                .map(|(repo_id, repo_name, _sot)| (repo_id, repo_name))
                .collect()),
            Err(e) => {
                let error_trace = format!("{e:#}");
                if error_trace.contains("UNIQUE constraint failed")
                    && error_trace.contains("git_repositories_source_of_truth.repo_name")
                {
                    Err(scs_errors::invalid_request(format!(
                        "Repo name should be unique but isn't. Details: {error_trace}"
                    ))
                    .into())
                } else {
                    Err(scs_errors::internal_error(format!(
                        "Failed to write row to git_repositories_source_of_truth. Details: {error_trace}"
                    ))
                    .into())
                }
            }
        }
    } else {
        Err(scs_errors::internal_error(
            "No rows in git_repositories_source_of_truth. That's unexpected",
        )
        .into())
    }
}

async fn create_repo_configs_in_mononoke() -> Result<(), scs_errors::ServiceError> {
    // TODO (Pierre) Implement
    Err(
        scs_errors::internal_error("Creating the repo in configerator is still unimplemented")
            .into(),
    )
}

async fn update_source_of_truth_to_mononoke(
    ctx: CoreContext,
    git_source_of_truth_config: &dyn GitSourceOfTruthConfig,
    params: &thrift::CreateReposParams,
) -> Result<(), scs_errors::ServiceError> {
    git_source_of_truth_config
        .update_source_of_truth_by_repo_names(
            &ctx,
            GitSourceOfTruth::Mononoke,
            &params
                .repos
                .iter()
                .map(|request| RepositoryName(request.repo_name.clone()))
                .collect::<Vec<_>>(),
        )
        .await
        .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?;
    Ok(())
}

#[cfg(fbcode_build)]
async fn create_repos_in_mononoke(
    ctx: CoreContext,
    git_source_of_truth_config: &dyn GitSourceOfTruthConfig,
    params: &thrift::CreateReposParams,
) -> Result<(), scs_errors::ServiceError> {
    // ## What:
    // Create these repositories in Mononoke.
    //
    // ## How:
    // * a) Mark these repositories as reserved in the git_repositories_source_of_truth table. This
    //      will ensure that another instance of `create_repos` won't compete with us on creating these
    //      repositories
    // * b) At this stage, we have reserved the repo ids we need, so we're not in contention
    //   anymore. We have time to do slow things, such as configuring these repositories in
    //   configerator to add the to Mononoke.
    //   Use configo to make a diff adding these repositories to quick_repo_definitions using
    //   the parameters passed in to `create_repos`via thrift
    //   Note: for authoring a config change, the configo API is what I need:
    //   https://www.internalfb.com/intern/rustdoc/fbcode/common/rust/configo:configo/configo/index.html
    //   Create the mutation, validate it and land it.
    // * c) Repo creation was successful, overwrite "reserved" with "mononoke" for these
    //   repositories. At this point, the repos were created and empty, so it's OK for their Source
    //   Of Truth to be in Mononoke already, ahead of creating them in Metagit and allowing pushes.

    let _repo_ids_and_names = retry(
        |_| reserve_repos_ids(ctx.clone(), git_source_of_truth_config, params),
        Duration::from_millis(1_000),
    )
    .binary_exponential_backoff()
    .max_attempts(5)
    .retry_if(|_attempt, error| match error {
        // No need to retry on request error. Let's report back to the user
        scs_errors::ServiceError::Request(_) => false,
        // Internal error can indicate a db error or contention on the ids. Retry with exponential
        // backoff
        _ => true,
    })
    .await?;

    // We have reserved the repo ids. Now it's time to actually create the repos, safe in the
    // knowledge that no-one will compete with us
    create_repo_configs_in_mononoke().await?;

    retry(
        |_| update_source_of_truth_to_mononoke(ctx.clone(), git_source_of_truth_config, params),
        Duration::from_millis(1_000),
    )
    .binary_exponential_backoff()
    .max_attempts(5)
    .await?;

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
        ensure_acls_allow_repo_creation(ctx.clone(), &params.repos, self.acl_provider.as_ref())
            .await?;
        if justknobs::eval("scm/mononoke:scs_create_repos_in_mononoke", None, None)
            .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?
        {
            update_repos_acls(ctx.clone(), &params).await?;
            create_repos_in_mononoke(ctx, self.git_source_of_truth_config.as_ref(), &params)
                .await?;
        }
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
