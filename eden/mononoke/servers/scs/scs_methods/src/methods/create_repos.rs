/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use auth_consts::AUTH_SET;
use auth_consts::ONCALL;
use auth_consts::ONCALL_GROUP_TYPE;
use auth_consts::REPO;
use auth_consts::SANDCASTLE_CMD;
use auth_consts::SANDCASTLE_TAG;
use auth_consts::SERVICE_IDENTITY;
#[cfg(fbcode_build)]
use configo::ConfigoClient;
use configo::mutation::Mutation;
use configo_thrift_srclients::ConfigoServiceClient;
use configo_thrift_srclients::make_ConfigoService_srclient;
use configo_thrift_srclients::thrift::MutationState;
use context::CoreContext;
use futures::future::try_join_all;
use futures_retry::retry;
use git_source_of_truth::GitSourceOfTruth;
use git_source_of_truth::GitSourceOfTruthConfig;
use git_source_of_truth::RepositoryName;
use git_source_of_truth::Staleness;
use infrasec_authorization::ACL;
use infrasec_authorization::Identity;
use infrasec_authorization::consts as auth_consts;
use infrasec_authorization_review::AclChange;
use infrasec_authorization_review::AclMetadata;
use infrasec_authorization_review::AclPermissionChange;
use infrasec_authorization_review::ChangeContents;
use infrasec_authorization_review::ChangeOperation;
use infrasec_authorization_review::EntryChange;
use infrasec_authorization_service::CommitChangeSpecificationRequest;
use infrasec_authorization_service_srclients::make_AuthorizationService_srclient;
use infrasec_authorization_service_srclients::thrift::ChangeSpecification;
use infrasec_authorization_service_srclients::thrift::errors::AsNoConfigExistsException;
use mononoke_api::MononokeError;
use mononoke_api::RepositoryId;
use mononoke_macros::mononoke;
use oncall::OncallClient;
use permission_checker::AclProvider;
use repo_authorization::AuthorizationContext;
use repos::QuickRepoDefinition;
use repos::QuickRepoDefinitionShardingConfig;
use repos::QuickRepoDefinitionTShirtSize;
use repos::RawCommitIdentityScheme;
use repos::RepoSpec;
use repos::ShardingRegions;
use repos::TShirtSize;
use sha2::Digest;
use sha2::Sha256;
use source_control as thrift;
use thrift::RepoSizeBucket;
use tracing::info;
use tracing::warn;

use crate::source_control_impl::SourceControlServiceImpl;

const DIFF_AUTHOR: &str = "scm_server_infra";
const REPO_DEFINITIONS_BASE_PATH: &str = "source/scm/mononoke/repos/definitions";
const REPO_DEFINITION_THRIFT_TYPE: &str = "QuickRepoDefinition";
const REPO_DEFINITION_THRIFT_PATH: &str = "source/scm/mononoke/repos/repos.thrift";
const REPO_SPEC_BASE_PATH: &str = "source/scm/mononoke/repos/git";
const REPO_SPEC_THRIFT_TYPE: &str = "RepoSpec";
const REPO_SPEC_THRIFT_PATH: &str = "source/scm/mononoke/repos/repos.thrift";

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
                (SANDCASTLE_TAG, "tpms_sandcastle_tag"),
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
        operation: ChangeOperation::ADD,
        entry_changes: identities
            .into_iter()
            .map(|(id_type, id_data)| EntryChange {
                entry: Identity {
                    id_type: id_type.to_string(),
                    id_data: id_data.to_string(),
                    ..Default::default()
                },
                operation: ChangeOperation::ADD,
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
        operation: ChangeOperation::ADD,
        metadata_update: Some(AclMetadata {
            oncall: Some(oncall_name.to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let change_contents = ChangeContents {
        acl_changes: vec![acl_change],
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
        let exists = thrift_client
            .groupExists(hipster_group)
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
                    && (entry.identity.id_type == AUTH_SET
                        || entry.identity.id_type == ONCALL
                        || entry.identity.id_type == ONCALL_GROUP_TYPE)
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
    format!("repos/git/{}", repo_name)
}

#[cfg(fbcode_build)]
fn make_top_level_acl_name_from_repo_name(repo_name: &str) -> String {
    let (top_level, _rest) = repo_name.split_once('/').unwrap_or((repo_name, ""));
    format!("repos/git/{}", top_level)
}

#[cfg(fbcode_build)]
async fn validate_and_process_custom_acl(
    ctx: CoreContext,
    repo_creation_request: &thrift::RepoCreationRequest,
    custom_acl: &thrift::CustomAclParams,
    valid_oncall_names_cache: &mut HashSet<String>,
    valid_hipster_groups_cache: &mut HashSet<String>,
) -> Result<(), scs_errors::ServiceError> {
    let acl_name = make_full_acl_name_from_repo_name(&repo_creation_request.repo_name);

    if !is_valid_oncall_name(
        ctx.clone(),
        &repo_creation_request.oncall_name,
        valid_oncall_names_cache,
    )
    .await?
    {
        return Err(scs_errors::invalid_request(format!(
            "Invalid oncall name: {}",
            repo_creation_request.oncall_name
        ))
        .into());
    }

    if !is_valid_hipster_group(
        ctx.clone(),
        &custom_acl.hipster_group,
        valid_hipster_groups_cache,
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
    } else {
        create_repo_acl(
            ctx,
            &acl_name,
            &repo_creation_request.oncall_name,
            &custom_acl.hipster_group,
        )
        .await?;
    }

    Ok(())
}

#[cfg(fbcode_build)]
async fn validate_top_level_acl_exists(
    ctx: CoreContext,
    repo_name: &str,
) -> Result<(), scs_errors::ServiceError> {
    let acl_name = make_top_level_acl_name_from_repo_name(repo_name);
    if try_fetching_repo_acl(ctx, &acl_name).await?.is_none() {
        return Err(scs_errors::invalid_request(format!(
            "Top level acl {acl_name} does not exist!"
        ))
        .into());
    }
    Ok(())
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
        if let Some(custom_acl) = &repo_creation_request.custom_acl {
            validate_and_process_custom_acl(
                ctx.clone(),
                repo_creation_request,
                custom_acl,
                &mut valid_oncall_names_cache,
                &mut valid_hipster_groups_cache,
            )
            .await?;
        } else {
            validate_top_level_acl_exists(ctx.clone(), &repo_creation_request.repo_name).await?;
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
) -> Result<Vec<(RepositoryId, thrift::RepoCreationRequest)>, scs_errors::ServiceError> {
    let max_id = git_source_of_truth_config
        .get_max_id(&ctx)
        .await
        .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?;
    if let Some(max_id) = max_id {
        let mut repo_id = max_id.id();
        let repo_ids_and_requests = params
            .repos
            .iter()
            .map(|request| {
                repo_id += 1;
                (RepositoryId::new(repo_id), request.clone())
            })
            .collect::<Vec<_>>();
        let result = git_source_of_truth_config
            .insert_repos(
                &ctx,
                &repo_ids_and_requests
                    .iter()
                    .map(|(id, request)| {
                        (
                            id.clone(),
                            RepositoryName(request.repo_name.clone()),
                            GitSourceOfTruth::Reserved,
                        )
                    })
                    .collect::<Vec<_>>(),
            )
            .await;
        match result {
            Ok(_) => Ok(repo_ids_and_requests),
            Err(e) => {
                let error_trace = format!("{e:#}");
                // Match both SQLite ("UNIQUE constraint failed") and MySQL
                // ("Duplicate entry '...' for key 'repo_name_idx'") errors.
                let is_duplicate = (error_trace.contains("UNIQUE constraint failed")
                    && error_trace.contains("git_repositories_source_of_truth.repo_name"))
                    || (error_trace.contains("Duplicate entry")
                        && error_trace.contains("repo_name_idx"));
                if is_duplicate {
                    let mut details = Vec::new();
                    for (_id, request) in &repo_ids_and_requests {
                        let repo_name = RepositoryName(request.repo_name.clone());
                        match git_source_of_truth_config
                            .get_by_repo_name(&ctx, &repo_name, Staleness::MostRecent)
                            .await
                        {
                            Ok(Some(entry)) => match entry.source_of_truth {
                                GitSourceOfTruth::Reserved => {
                                    details.push(format!(
                                        "Repo '{}' (id={}) has a stale 'Reserved' entry from a prior failed creation attempt. \
                                         It is safe to delete this row and retry.",
                                        request.repo_name, entry.repo_id
                                    ));
                                }
                                ref sot => {
                                    details.push(format!(
                                        "DANGER: Repo '{}' (id={}) already exists with source_of_truth={}. \
                                         Do NOT force-create — this will cause split-brain! \
                                         See SEV S617275 for context.",
                                        request.repo_name, entry.repo_id, sot
                                    ));
                                }
                            },
                            Ok(None) => {
                                details.push(format!(
                                    "Repo '{}': UNIQUE constraint violated but no row found on lookup. \
                                     Original error: {error_trace}",
                                    request.repo_name
                                ));
                            }
                            Err(lookup_err) => {
                                details.push(format!(
                                    "Repo '{}': UNIQUE constraint violated but lookup failed: {:#}. \
                                     Original error: {error_trace}",
                                    request.repo_name, lookup_err
                                ));
                            }
                        }
                    }
                    Err(scs_errors::invalid_request(details.join("\n")).into())
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

fn to_tshirt_size(
    size_bucket: RepoSizeBucket,
) -> Result<QuickRepoDefinitionTShirtSize, scs_errors::ServiceError> {
    match size_bucket {
        // < 100 MB
        RepoSizeBucket::EXTRA_SMALL => Ok(QuickRepoDefinitionTShirtSize::SMALL),
        // < 1 GB | < 10 GB
        RepoSizeBucket::SMALL | RepoSizeBucket::MEDIUM => Ok(QuickRepoDefinitionTShirtSize::MEDIUM),
        // < 100 GB
        RepoSizeBucket::LARGE => Ok(QuickRepoDefinitionTShirtSize::LARGE),
        // >= 100 GB
        RepoSizeBucket::EXTRA_LARGE => Ok(QuickRepoDefinitionTShirtSize::HUGE),
        _ => Err(scs_errors::internal_error(format!(
            "Unsupported RepoSizeBucket: {size_bucket:?}"
        ))
        .into()),
    }
}

/// Generates the file path for a repo definition file.
/// Path format: source/scm/mononoke/repos/definitions/repo_{shard}/repo_{repoid}.cconf
/// where shard is the first two digits of the repo_id.
fn make_repo_definition_file_path(repo_id: &RepositoryId) -> String {
    let repo_id_str = repo_id.id().to_string();
    let shard = if repo_id_str.len() >= 2 {
        &repo_id_str[..2]
    } else {
        // For repo IDs less than 10, use "0" as shard
        "0"
    };

    format!(
        "{}/repo_{}/repo_{}.cconf",
        REPO_DEFINITIONS_BASE_PATH,
        shard,
        repo_id.id()
    )
}

fn make_quick_repo_definition(
    (repo_id, request): &(RepositoryId, thrift::RepoCreationRequest),
) -> Result<QuickRepoDefinition, scs_errors::ServiceError> {
    Ok(QuickRepoDefinition {
        repo_id: repo_id.id(),
        repo_name: request.repo_name.clone(),
        config_tiers: vec![
            "gitimport".to_string(),
            "gitimport_content".to_string(),
            "scs".to_string(),
        ],
        enabled: true,
        readonly: false,
        default_commit_identity_scheme: RawCommitIdentityScheme::GIT,
        custom_repo_config: None,
        git_lfs_interpret_pointers: true,
        use_upstream_lfs_server: true,
        custom_storage_config: None,
        t_shirt_size: to_tshirt_size(request.size_bucket)?,
        sharding_config: QuickRepoDefinitionShardingConfig::BGM_ONLY_REGIONS,
        custom_acl_name: None,
        preloaded_commit_graph_blobstore_key: None,
        git_concurrency: None,
        enable_git_bundle_uri: None,
        ..Default::default()
    })
}

fn to_repo_spec_tshirt_size(
    size_bucket: RepoSizeBucket,
) -> Result<TShirtSize, scs_errors::ServiceError> {
    match size_bucket {
        RepoSizeBucket::EXTRA_SMALL => Ok(TShirtSize::SMALL),
        RepoSizeBucket::SMALL | RepoSizeBucket::MEDIUM => Ok(TShirtSize::MEDIUM),
        RepoSizeBucket::LARGE => Ok(TShirtSize::LARGE),
        RepoSizeBucket::EXTRA_LARGE => Ok(TShirtSize::HUGE),
        _ => Err(scs_errors::internal_error(format!(
            "Unsupported RepoSizeBucket: {size_bucket:?}"
        ))
        .into()),
    }
}

/// Generates the file path for a RepoSpec file.
/// Path format: source/scm/mononoke/repos/git/{hash_dir}/{hash_prefix}_{repo_name_escaped}.cconf
/// where hash_dir is the first 2 hex chars and hash_prefix is the first 8 hex
/// chars of SHA-256(repo_name).
/// The hash_dir distributes files across 256 subdirectories to avoid
/// configerator directory size limits. The hash_prefix in the filename
/// prevents collisions between repos that differ only in '/' vs '_'
/// (e.g., "org/repo" vs "org_repo" both escape to "org_repo" but have
/// different hashes).
fn make_repo_spec_file_path(repo_name: &str) -> String {
    let hash = Sha256::digest(repo_name.as_bytes());
    let hash_dir = format!("{:02x}", hash[0]);
    let hash_prefix = format!(
        "{:02x}{:02x}{:02x}{:02x}",
        hash[0], hash[1], hash[2], hash[3]
    );
    format!(
        "{}/{}/{}_{}.cconf",
        REPO_SPEC_BASE_PATH,
        hash_dir,
        hash_prefix,
        repo_name.replace('/', "_")
    )
}

fn make_repo_spec(
    (repo_id, request): &(RepositoryId, thrift::RepoCreationRequest),
) -> Result<RepoSpec, scs_errors::ServiceError> {
    Ok(RepoSpec {
        repo_id: repo_id.id(),
        repo_name: request.repo_name.clone(),
        hipster_acl: make_full_acl_name_from_repo_name(&request.repo_name),
        enabled: true,
        readonly: false,
        default_commit_identity_scheme: RawCommitIdentityScheme::GIT,
        enable_git_bundle_uri: None,
        tiers: vec![
            "gitimport".to_string(),
            "gitimport_content".to_string(),
            "scs".to_string(),
        ],
        t_shirt_size: to_repo_spec_tshirt_size(request.size_bucket)?,
        sharding_regions: ShardingRegions::BGM_ONLY_REGIONS,
        repo_config: None,
        tier_overrides: None,
        ..Default::default()
    })
}

async fn prepare_repo_configs_mutation_nowait(
    ctx: CoreContext,
    repos_ids_and_requests: Vec<(RepositoryId, thrift::RepoCreationRequest)>,
) -> Result<i64, scs_errors::ServiceError> {
    let configo_client = ConfigoClient::with_client(
        ctx.fb,
        make_ConfigoService_srclient!(ctx.fb)
            .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?,
    );
    let mut txn = configo_client.managed_transaction();

    let use_repo_spec = justknobs::eval("scm/mononoke:create_repos_use_repo_spec", None, None)
        .map_err(scs_errors::internal_error)?;

    // Create individual repo config files
    for (repo_id, request) in &repos_ids_and_requests {
        if use_repo_spec {
            let repo_spec = make_repo_spec(&(*repo_id, request.clone()))?;
            let file_path = make_repo_spec_file_path(&request.repo_name);
            txn.set_thrift_object(
                repo_spec,
                file_path,
                REPO_SPEC_THRIFT_TYPE.to_string(),
                REPO_SPEC_THRIFT_PATH.to_string(),
                None,
            );
        } else {
            let repo_definition = make_quick_repo_definition(&(*repo_id, request.clone()))?;
            let file_path = make_repo_definition_file_path(repo_id);
            txn.set_thrift_object(
                repo_definition,
                file_path,
                REPO_DEFINITION_THRIFT_TYPE.to_string(),
                REPO_DEFINITION_THRIFT_PATH.to_string(),
                None,
            );
        }
    }

    let summary = repos_ids_and_requests
        .iter()
        .flat_map(|(repo_id, request)| format!("|{}|{}|", repo_id, request.repo_name).into_chars())
        .collect::<String>();
    let mutation = txn
        .prepare_mutation_request()
        .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?
        .add_author(DIFF_AUTHOR.to_string())
        .add_commit_message(
            format!(
                "[mononoke]: Create {} git repositories (automated)\n@bypass_size_limit",
                repos_ids_and_requests.len()
            ),
            summary,
        )
        .prepare_nowait()
        .await
        .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?;
    Ok(mutation.id)
}

async fn update_source_of_truth_to_mononoke_for_mutation_id(
    ctx: CoreContext,
    git_source_of_truth_config: &dyn GitSourceOfTruthConfig,
    mutation_id: i64,
) -> Result<(), scs_errors::ServiceError> {
    git_source_of_truth_config
        .update_source_of_truth_by_mutation_id(&ctx, GitSourceOfTruth::Mononoke, mutation_id)
        .await
        .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?;
    Ok(())
}

async fn update_mutation_id_by_repo_names_for_reserved_repos(
    ctx: CoreContext,
    git_source_of_truth_config: &dyn GitSourceOfTruthConfig,
    params: &thrift::CreateReposParams,
    mutation_id: i64,
) -> Result<(), scs_errors::ServiceError> {
    git_source_of_truth_config
        .update_mutation_id_by_repo_names_for_reserved_repos(
            &ctx,
            &params
                .repos
                .iter()
                .map(|request| RepositoryName(request.repo_name.clone()))
                .collect::<Vec<_>>(),
            mutation_id,
        )
        .await
        .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?;
    Ok(())
}

async fn delete_source_of_truth_for_reserved_repos(
    ctx: CoreContext,
    git_source_of_truth_config: &dyn GitSourceOfTruthConfig,
    params: &thrift::CreateReposParams,
) -> Result<(), scs_errors::ServiceError> {
    git_source_of_truth_config
        .delete_source_of_truth_by_repo_names_for_reserved_repos(
            &ctx,
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

async fn delete_source_of_truth_for_mutation_id(
    ctx: CoreContext,
    git_source_of_truth_config: &dyn GitSourceOfTruthConfig,
    mutation_id: i64,
) -> Result<(), scs_errors::ServiceError> {
    git_source_of_truth_config
        .delete_source_of_truth_for_mutation_id(&ctx, &mutation_id)
        .await
        .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?;
    Ok(())
}

#[cfg(fbcode_build)]
async fn create_repos_in_mononoke(
    ctx: CoreContext,
    git_source_of_truth_config: Arc<dyn GitSourceOfTruthConfig>,
    params: &thrift::CreateReposParams,
) -> Result<Option<i64>, scs_errors::ServiceError> {
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
    // * c) Repo creation was successful, overwrite "reserved" with "mononoke" for these
    //   repositories. At this point, the repos were created and empty, so it's OK for their Source
    //   Of Truth to be in Mononoke already, ahead of creating them in Metagit and allowing pushes.

    let (repo_ids_and_requests, _attempts) = retry(
        |_| reserve_repos_ids(ctx.clone(), git_source_of_truth_config.as_ref(), params),
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
    match prepare_repo_configs_mutation_nowait(ctx.clone(), repo_ids_and_requests).await {
        Ok(mutation_id) => {
            retry(
                |_| {
                    update_mutation_id_by_repo_names_for_reserved_repos(
                        ctx.clone(),
                        git_source_of_truth_config.as_ref(),
                        params,
                        mutation_id,
                    )
                },
                Duration::from_millis(1_000),
            )
            .binary_exponential_backoff()
            .max_attempts(5)
            .await?;

            let spawn_task =
                justknobs::eval("scm/mononoke:spawn_mutation_polling_task", None, None)
                    .map_err(scs_errors::internal_error)?;
            if spawn_task {
                // Clone necessary data for the spawned task
                let poll_ctx = ctx.clone();
                let git_sot_config = git_source_of_truth_config.clone();

                mononoke::spawn_task({
                    async move {
                        // Poll interval - start with 5 seconds
                        let poll_interval = Duration::from_secs(5);

                        loop {
                            match poll_mutation_id(
                                poll_ctx.clone(),
                                git_sot_config.as_ref(),
                                mutation_id,
                            )
                            .await
                            {
                                Ok(state) => match state {
                                    MutationState::PREPARED
                                    | MutationState::CANARYING
                                    | MutationState::PREPARING
                                    | MutationState::LANDING
                                    | MutationState::SERVICE_CANARYING
                                    | MutationState::VALIDATING => {
                                        info!(
                                            "mutation in progress for mutation_id {}",
                                            mutation_id
                                        );
                                        tokio::time::sleep(poll_interval).await;
                                    }
                                    _ => break,
                                },
                                Err(e) => {
                                    warn!(
                                        "Error polling repo creation for mutation_id: {}, error: {:?}",
                                        mutation_id, e
                                    );
                                    break;
                                }
                            }
                        }
                    }
                });
            }
            Ok(Some(mutation_id))
        }
        Err(e) => {
            // We failed to land the mutation, so it is safe to "release the lock" on these repo
            // ids and names, which will allow a future attempt to succeed.
            retry(
                |_| {
                    delete_source_of_truth_for_reserved_repos(
                        ctx.clone(),
                        git_source_of_truth_config.as_ref(),
                        params,
                    )
                },
                Duration::from_millis(1_000),
            )
            .binary_exponential_backoff()
            .max_attempts(5)
            .await?;
            Err(e)
        }
    }
}

#[cfg(not(fbcode_build))]
async fn create_repos_in_mononoke(
    _ctx: CoreContext,
    _git_source_of_truth_config: Arc<dyn GitSourceOfTruthConfig>,
    _params: &thrift::CreateReposParams,
) -> Result<(), scs_errors::ServiceError> {
    println!("No access to configo in oss build");
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
        update_repos_acls(ctx.clone(), &params).await?;
        let mutation_id =
            create_repos_in_mononoke(ctx, self.git_source_of_truth_config.clone(), &params).await?;

        Ok(thrift::CreateReposToken {
            mutation_id,
            ..Default::default()
        })
    }

    #[cfg(fbcode_build)]
    pub(crate) async fn create_repos_poll(
        &self,
        ctx: CoreContext,
        token: thrift::CreateReposToken,
    ) -> Result<thrift::CreateReposPollResponse, scs_errors::ServiceError> {
        if token.mutation_id.is_none() {
            return Err(scs_errors::invalid_request("mutation_id is not set".to_string()).into());
        }

        let mutation_id = token.mutation_id.unwrap();
        let mutation_state;
        let status = match poll_mutation_id(
            ctx,
            self.git_source_of_truth_config.as_ref(),
            mutation_id,
        )
        .await
        {
            Ok(state) => {
                mutation_state = state;
                match state {
                    MutationState::LANDED => thrift::CreateReposStatus::SUCCESS,
                    MutationState::FAILED => thrift::CreateReposStatus::FAILED,
                    MutationState::ABORTED => thrift::CreateReposStatus::ABORTED,
                    MutationState::PREPARED
                    | MutationState::CANARYING
                    | MutationState::PREPARING
                    | MutationState::LANDING
                    | MutationState::SERVICE_CANARYING
                    | MutationState::VALIDATING => thrift::CreateReposStatus::IN_PROGRESS,
                    _ => {
                        return Err(scs_errors::internal_error(format!(
                            "Unexpected Configo mutation state: {}",
                            state
                        ))
                        .into());
                    }
                }
            }
            Err(err) => return Err(err),
        };

        let message = Some(format!("Mutation state: {}", mutation_state));
        Ok(thrift::CreateReposPollResponse {
            result: Some(thrift::CreateReposResponse {
                status,
                message,
                ..Default::default()
            }),
            ..Default::default()
        })
    }
}

#[cfg(fbcode_build)]
async fn poll_mutation_id(
    ctx: CoreContext,
    git_source_of_truth_config: &dyn GitSourceOfTruthConfig,
    mutation_id: i64,
) -> Result<MutationState, scs_errors::ServiceError> {
    match poll_mutation_id_impl(ctx.clone(), git_source_of_truth_config, mutation_id, 0).await {
        Ok(state) => Ok(state),
        Err(e) => match e {
            scs_errors::ServiceError::Poll(_) => {
                poll_mutation_id_impl(ctx.clone(), git_source_of_truth_config, mutation_id, 1).await
            }
            _ => Err(e),
        },
    }
}

async fn handle_landed_state(
    ctx: CoreContext,
    git_source_of_truth_config: &dyn GitSourceOfTruthConfig,
    mutation_id: i64,
) -> Result<(), scs_errors::ServiceError> {
    retry(
        |_| {
            update_source_of_truth_to_mononoke_for_mutation_id(
                ctx.clone(),
                git_source_of_truth_config,
                mutation_id,
            )
        },
        Duration::from_millis(1_000),
    )
    .binary_exponential_backoff()
    .max_attempts(5)
    .await?;
    Ok(())
}

async fn handle_prepared_state(
    ctx: CoreContext,
    configo_client: ConfigoServiceClient,
    mutation_id: i64,
    is_signed: bool,
    retry_count: i64,
    error_message: String,
) -> Result<(), scs_errors::ServiceError> {
    if !is_signed {
        if let Err(e) = initiate_land_for_mutation(ctx, configo_client, mutation_id).await {
            if retry_count == 0 {
                return Err(scs_errors::poll_error(format!(
                    "Configo mutation error: {}",
                    error_message
                ))
                .into());
            } else {
                return Err(e);
            }
        }
    }
    Ok(())
}

async fn handle_mutation_state(
    ctx: CoreContext,
    git_source_of_truth_config: &dyn GitSourceOfTruthConfig,
    configo_client: ConfigoServiceClient,
    state: MutationState,
    mutation_id: i64,
    is_signed: bool,
    retry_count: i64,
    error_message: String,
) -> Result<(), scs_errors::ServiceError> {
    match state {
        MutationState::LANDED => {
            handle_landed_state(ctx, git_source_of_truth_config, mutation_id).await?;
        }
        MutationState::FAILED | MutationState::ABORTED => {
            cleanup_repos(ctx, git_source_of_truth_config, mutation_id).await?;
        }
        MutationState::PREPARED => {
            handle_prepared_state(
                ctx,
                configo_client,
                mutation_id,
                is_signed,
                retry_count,
                error_message,
            )
            .await?;
        }
        _ => (),
    }
    Ok(())
}

async fn poll_mutation_id_impl(
    ctx: CoreContext,
    git_source_of_truth_config: &dyn GitSourceOfTruthConfig,
    mutation_id: i64,
    retry_count: i64,
) -> std::result::Result<MutationState, scs_errors::ServiceError> {
    let configo_client = make_ConfigoService_srclient!(ctx.fb)
        .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?;

    let resp = configo_client
        .status(&mutation_id)
        .await
        .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?;

    if resp.error {
        cleanup_repos(ctx.clone(), git_source_of_truth_config, mutation_id).await?;
        return Err(scs_errors::internal_error(format!(
            "Configo mutation error: {}",
            resp.errorMessage
        ))
        .into());
    }

    let mutation = resp.mutation.ok_or_else(|| {
        warn!("mutation state is not present {}", mutation_id);
        scs_errors::internal_error(format!("Mutation state not available for {mutation_id}"))
    })?;

    if mutation.stateInfo.isError {
        info!("cleaning up, mutation state info has error {}", mutation_id);
        cleanup_repos(ctx.clone(), git_source_of_truth_config, mutation_id).await?;
        return Err(scs_errors::internal_error(format!(
            "Configo mutation error: {}",
            mutation.stateInfo.errorMessage
        ))
        .into());
    }

    let state = mutation.stateInfo.state;
    handle_mutation_state(
        ctx,
        git_source_of_truth_config,
        configo_client,
        state,
        mutation_id,
        mutation.stateInfo.isSigned,
        retry_count,
        mutation.stateInfo.errorMessage,
    )
    .await?;

    Ok(state)
}

async fn cleanup_repos(
    ctx: CoreContext,
    git_source_of_truth_config: &dyn GitSourceOfTruthConfig,
    mutation_id: i64,
) -> Result<(), scs_errors::ServiceError> {
    let (_result, _attempts) = retry(
        |_| {
            delete_source_of_truth_for_mutation_id(
                ctx.clone(),
                git_source_of_truth_config,
                mutation_id,
            )
        },
        Duration::from_millis(1_000),
    )
    .binary_exponential_backoff()
    .max_attempts(5)
    .await?;
    Ok(())
}

#[cfg(fbcode_build)]
async fn initiate_land_for_mutation(
    ctx: CoreContext,
    configo_client: ConfigoServiceClient,
    mutation_id: i64,
) -> Result<(), scs_errors::ServiceError> {
    let mutation = Mutation::new(ctx.fb, configo_client, mutation_id);

    let mutation_id = mutation
        .land_nowait()
        .await
        .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?;

    info!("initiated land for mutation id  {}", mutation_id.id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn test_make_repo_definition_file_path_large_id() {
        // Test with a large repo ID (5 digits)
        let repo_id = RepositoryId::new(12345);
        let path = make_repo_definition_file_path(&repo_id);
        assert_eq!(
            path,
            "source/scm/mononoke/repos/definitions/repo_12/repo_12345.cconf"
        );
    }

    #[mononoke::test]
    fn test_make_repo_definition_file_path_three_digit_id() {
        // Test with a three-digit repo ID
        let repo_id = RepositoryId::new(456);
        let path = make_repo_definition_file_path(&repo_id);
        assert_eq!(
            path,
            "source/scm/mononoke/repos/definitions/repo_45/repo_456.cconf"
        );
    }

    #[mononoke::test]
    fn test_make_repo_definition_file_path_single_digit_id() {
        // Test with a single-digit repo ID (should use "0" as shard)
        let repo_id = RepositoryId::new(5);
        let path = make_repo_definition_file_path(&repo_id);
        assert_eq!(
            path,
            "source/scm/mononoke/repos/definitions/repo_0/repo_5.cconf"
        );
    }

    #[mononoke::test]
    fn test_make_repo_spec_file_path_simple_name() {
        let path = make_repo_spec_file_path("my-repo");
        assert!(
            path.starts_with("source/scm/mononoke/repos/git/"),
            "Path should start with RepoSpec base path: {path}"
        );
        // Filename should include hash prefix and repo name
        assert!(
            path.contains("_my-repo.cconf"),
            "Path should end with hash_repo-name.cconf: {path}"
        );
    }

    #[mononoke::test]
    fn test_make_repo_spec_file_path_slash_in_name() {
        let path = make_repo_spec_file_path("org/project/repo");
        assert!(
            path.contains("_org_project_repo.cconf"),
            "Slashes should be replaced with underscores: {path}"
        );
    }

    #[mononoke::test]
    fn test_make_repo_spec_file_path_no_collision_slash_vs_underscore() {
        let path1 = make_repo_spec_file_path("org/repo");
        let path2 = make_repo_spec_file_path("org_repo");
        assert_ne!(
            path1, path2,
            "Repos differing only in '/' vs '_' must produce different paths"
        );
    }

    #[mononoke::test]
    fn test_make_repo_spec_file_path_deterministic() {
        let path1 = make_repo_spec_file_path("test/repo");
        let path2 = make_repo_spec_file_path("test/repo");
        assert_eq!(path1, path2, "Hash-based path should be deterministic");
    }

    #[mononoke::test]
    fn test_make_repo_spec_file_path_different_repos_may_differ() {
        let path1 = make_repo_spec_file_path("repo-alpha");
        let path2 = make_repo_spec_file_path("repo-beta");
        assert_ne!(
            path1, path2,
            "Different repos should produce different paths"
        );
    }

    #[mononoke::test]
    fn test_to_repo_spec_tshirt_size_mapping() {
        assert_eq!(
            to_repo_spec_tshirt_size(RepoSizeBucket::EXTRA_SMALL).unwrap(),
            TShirtSize::SMALL
        );
        assert_eq!(
            to_repo_spec_tshirt_size(RepoSizeBucket::SMALL).unwrap(),
            TShirtSize::MEDIUM
        );
        assert_eq!(
            to_repo_spec_tshirt_size(RepoSizeBucket::MEDIUM).unwrap(),
            TShirtSize::MEDIUM
        );
        assert_eq!(
            to_repo_spec_tshirt_size(RepoSizeBucket::LARGE).unwrap(),
            TShirtSize::LARGE
        );
        assert_eq!(
            to_repo_spec_tshirt_size(RepoSizeBucket::EXTRA_LARGE).unwrap(),
            TShirtSize::HUGE
        );
    }

    #[mononoke::test]
    fn test_make_repo_spec_produces_valid_spec() {
        let repo_id = RepositoryId::new(12345);
        let request = thrift::RepoCreationRequest {
            repo_name: "org/my-repo".to_string(),
            size_bucket: RepoSizeBucket::SMALL,
            ..Default::default()
        };

        let spec = make_repo_spec(&(repo_id, request)).expect("make_repo_spec should succeed");

        assert_eq!(spec.repo_id, 12345);
        assert_eq!(spec.repo_name, "org/my-repo");
        assert!(spec.enabled);
        assert!(!spec.readonly);
        assert_eq!(
            spec.default_commit_identity_scheme,
            RawCommitIdentityScheme::GIT
        );
        assert_eq!(spec.t_shirt_size, TShirtSize::MEDIUM);
        assert_eq!(spec.sharding_regions, ShardingRegions::BGM_ONLY_REGIONS);
        assert_eq!(spec.tiers, vec!["gitimport", "gitimport_content", "scs"]);
        assert!(
            spec.repo_config.is_none(),
            "New repos should have no custom config"
        );
        assert!(
            spec.tier_overrides.is_none(),
            "New repos should have no tier overrides"
        );
        assert!(
            !spec.hipster_acl.is_empty(),
            "hipster_acl should be derived from repo name"
        );
    }
}
