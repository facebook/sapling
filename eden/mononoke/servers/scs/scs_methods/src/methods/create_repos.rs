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
use fbinit::FacebookInit;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::future::try_join_all;
use futures::stream;
use futures_retry::retry;
use git_source_of_truth::GitSourceOfTruth;
use git_source_of_truth::GitSourceOfTruthConfig;
use git_source_of_truth::RepositoryName;
use git_source_of_truth::Staleness;
use git_symbolic_refs::GitSymbolicRefs;
use git_symbolic_refs::GitSymbolicRefsEntry;
use git_symbolic_refs::SqlGitSymbolicRefsBuilder;
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
use metaconfig_parser::configerator_repo_config_handle;
use mononoke_api::MononokeError;
use mononoke_api::RepositoryId;
use mononoke_configs::MononokeConfigs;
use mononoke_macros::mononoke;
use oncall::OncallClient;
use permission_checker::AclProvider;
use repo_authorization::AuthorizationContext;
use repo_spec_writer::RepoIndexEntry;
use repo_spec_writer::RepoSpecDir;
use repo_spec_writer::append_to_repo_index;
use repo_spec_writer::make_repo_spec_config_path;
use repo_spec_writer::make_repo_spec_file_path;
use repo_spec_writer::tier_list_for_repo_spec;
use repos::RawCommitIdentityScheme;
use repos::RawRepoConfig;
use repos::RepoSpec;
use repos::ShardingRegions;
use repos::TShirtSize;
use source_control as thrift;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::facebook::MysqlOptions;
use thrift::RepoSizeBucket;
use tokio::sync::OnceCell;
use tracing::info;
use tracing::warn;

use crate::source_control_impl::SourceControlServiceImpl;

const DIFF_AUTHOR: &str = "scm_server_infra";
const REPO_SPEC_THRIFT_TYPE: &str = "RepoSpec";
const REPO_SPEC_THRIFT_PATH: &str = "source/scm/mononoke/repos/repos.thrift";
/// JustKnob gating the attach-to-in-flight-mutation idempotency path in
/// `reserve_repos_ids`. Shared by the production call site and the tests so
/// the two can never drift.
const ATTACH_JK: &str = "scm/mononoke:create_repos_attach_to_inflight_mutation";
/// Whether an out-of-bounds batch is rejected or merely logged. Turning this
/// off is the kill switch; the caps themselves are not runtime-tunable.
/// Switchval'd on the tier name, so enforcement can be turned on one tier at a
/// time.
const ENFORCE_BATCH_SIZE_JK: &str = "scm/mononoke:create_repos_enforce_max_batch_size";
const WRITE_DEFAULT_BRANCH_SYMREF_JK: &str =
    "scm/mononoke:create_repos_write_default_branch_symref";
const DEFAULT_GIT_REPO_CONFIG_PATH: &str = "scm/mononoke/repos/common/default_git_repo_config";

/// Group granting the elevated batch tier, one step below Source Control's own.
/// Membership is the whole mechanism:
/// callers are added and removed there rather than here, so widening or
/// narrowing who may create repos in bulk does not need a diff. Expected
/// members are the automated bundle-import pipeline
/// (`SANDCASTLE_TAG:git_repo_importer`, the tag its Skycastle workflow declares
/// in `required_tag_identities`) and whoever is currently doing bulk onboarding
/// by hand.
const BULK_REPO_CREATORS_GROUP: &str = "bulk_repo_creators";

/// Which batch-size cap applies to a caller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BatchSizeTier {
    SourceControl,
    /// Members of the bulk-repo-creators group.
    Elevated,
    Default,
}

impl BatchSizeTier {
    /// Enforcement switchval, and the tier as it appears in the advisory log.
    fn name(self) -> &'static str {
        match self {
            Self::SourceControl => "source_control",
            Self::Elevated => "elevated",
            Self::Default => "default",
        }
    }

    /// Only Source Control's tier covers the largest batches on record. A
    /// delegated grant is deliberately smaller than what the team that hands it
    /// out can do itself -- see the tests, which pin every rejection.
    fn max_batch_size(self) -> usize {
        match self {
            Self::SourceControl => 1000,
            Self::Elevated => 500,
            Self::Default => 50,
        }
    }
}

/// Resolve the caller's batch-size tier.
///
/// A property of the caller, not of the request: the same identity gets the
/// same cap whatever it is creating.
///
/// Elevated is group membership rather than anything hardcoded here, so the
/// set of bulk creators can change without a code change.
///
/// Keyed on the mTLS identity set, deliberately **not** on the `client_id`
/// header: that header is client-supplied and never verified against the
/// identities (`source_control_impl.rs` sets it on the untrusted-proxy branch
/// too), so it cannot carry an authorization decision. It is not keyed on an
/// ACL action either -- a Hipster action expresses a permission, and no ACL
/// carries one for a quota; the closest in-tree precedent for a per-caller
/// limit is a rate-limit target on an identity set
/// (`rate_limiting/src/config.rs`).
///
/// Fails *down*, never up -- a group lookup error yields the lower tier.
async fn batch_size_tier_for_caller(
    ctx: &CoreContext,
    acl_provider: &dyn AclProvider,
) -> BatchSizeTier {
    let identities = ctx.metadata().identities();

    // Checked before the group so that a Source Control member who is also in
    // it gets the higher of the two caps rather than whichever was looked up
    // first. It also means an edit that empties or breaks the group cannot
    // leave the oncall holding an unrecognised caller's cap while they fix it.
    if let Ok(admins) = acl_provider.admin_group().await {
        if admins.is_member(identities).await {
            return BatchSizeTier::SourceControl;
        }
    }

    if let Ok(bulk_creators) = acl_provider.group(BULK_REPO_CREATORS_GROUP).await {
        if bulk_creators.is_member(identities).await {
            return BatchSizeTier::Elevated;
        }
    }

    BatchSizeTier::Default
}

/// Classify a batch against a cap, without deciding whether to act on it.
///
/// The error must be a `Request` error: `create_repos_in_mononoke` retries
/// everything that is not `ServiceError::Request`, so an internal error here
/// would be silently retried instead of reported to the caller.
fn check_batch_size(count: usize, max_batch_size: usize) -> Result<(), scs_errors::ServiceError> {
    if count == 0 {
        return Err(scs_errors::invalid_request(
            "create_repos was called with an empty batch, which creates nothing but still costs a \
             configerator mutation. Omit the call instead."
                .to_string(),
        )
        .into());
    }

    if count <= max_batch_size {
        return Ok(());
    }

    Err(scs_errors::invalid_request(format!(
        "create_repos was asked to create {count} repos, which exceeds the maximum batch size of \
         {max_batch_size} for this caller. Split the request into smaller batches, or ask Source \
         Control for membership of the '{BULK_REPO_CREATORS_GROUP}' group."
    ))
    .into())
}

const HEAD_SYMREF: &str = "HEAD";

fn validate_default_branch(branch: &str) -> Result<&str, scs_errors::ServiceError> {
    if branch.is_empty() {
        return Err(scs_errors::invalid_request(
            "default_branch must not be empty; omit the field to create a repo without a HEAD \
             symref"
                .to_string(),
        )
        .into());
    }
    if branch.eq_ignore_ascii_case(HEAD_SYMREF) {
        return Err(scs_errors::invalid_request(
            "default_branch must not be 'HEAD' (in any casing): HEAD is the symref that points \
             at the default branch, not a branch itself"
                .to_string(),
        )
        .into());
    }
    if branch.starts_with("refs/") {
        return Err(scs_errors::invalid_request(format!(
            "default_branch must be a short branch name (e.g. 'main'), not a full ref: '{branch}'"
        ))
        .into());
    }
    Ok(branch)
}

/// Gate for the `default_branch` feature, evaluated once per batch: returns
/// whether symref writes are enabled, validating every set `default_branch`
/// when they are. The JK is never evaluated when no request sets the field,
/// and with it off the field is fully inert (no validation, no writes).
fn validate_default_branches(
    repos: &[thrift::RepoCreationRequest],
) -> Result<bool, scs_errors::ServiceError> {
    if !repos.iter().any(|request| request.default_branch.is_some()) {
        return Ok(false);
    }
    if !justknobs::eval(WRITE_DEFAULT_BRANCH_SYMREF_JK, None, None) {
        return Ok(false);
    }
    for request in repos {
        if let Some(branch) = &request.default_branch {
            validate_default_branch(branch)?;
        }
    }
    Ok(true)
}

async fn write_default_branch_symref(
    ctx: &CoreContext,
    store: &dyn GitSymbolicRefs,
    branch: &str,
) -> Result<(), scs_errors::ServiceError> {
    let entry = GitSymbolicRefsEntry::new(
        HEAD_SYMREF.to_string(),
        branch.to_string(),
        "branch".to_string(),
    )
    .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?;
    store
        .add_or_update_entries(ctx, vec![entry])
        .await
        .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?;
    Ok(())
}

#[async_trait::async_trait]
trait SymrefStoreProvider: Send + Sync {
    async fn repo_store(
        &self,
        repo_id: RepositoryId,
    ) -> Result<Arc<dyn GitSymbolicRefs>, scs_errors::ServiceError>;
}

#[derive(Clone)]
struct SymrefStoreFactory {
    fb: FacebookInit,
    configs: Arc<MononokeConfigs>,
    mysql_options: MysqlOptions,
    /// Storage-name/config resolution and SQL connection setup happen once
    /// per factory (one factory per request); per-repo stores share the
    /// connections.
    builder: Arc<OnceCell<SqlGitSymbolicRefsBuilder>>,
}

impl SymrefStoreFactory {
    fn default_git_storage_name(&self) -> Result<String, scs_errors::ServiceError> {
        let config_store = self.configs.config_store().ok_or_else(|| {
            scs_errors::internal_error(
                "No config store available for resolving the default git repo storage",
            )
        })?;
        let template = configerator_repo_config_handle(DEFAULT_GIT_REPO_CONFIG_PATH, config_store)
            .map_err(|e| {
                scs_errors::internal_error(format!("Failed to load default git repo config: {e:#}"))
            })?
            .get();
        template.storage_config.clone().ok_or_else(|| {
            scs_errors::internal_error(format!(
                "{DEFAULT_GIT_REPO_CONFIG_PATH} does not name a storage config"
            ))
            .into()
        })
    }

    async fn shared_builder(&self) -> Result<&SqlGitSymbolicRefsBuilder, scs_errors::ServiceError> {
        self.builder
            .get_or_try_init(|| async {
                let storage_name = self.default_git_storage_name()?;
                let storage_configs = self.configs.storage_configs();
                let storage_config =
                    storage_configs.storage.get(&storage_name).ok_or_else(|| {
                        scs_errors::ServiceError::from(scs_errors::internal_error(format!(
                            "Storage config '{storage_name}' not found while building the symref \
                             store"
                        )))
                    })?;
                SqlGitSymbolicRefsBuilder::with_metadata_database_config(
                    self.fb,
                    &storage_config.metadata,
                    &self.mysql_options,
                    false,
                )
                .await
                .map_err(|e| {
                    scs_errors::ServiceError::from(scs_errors::internal_error(format!(
                        "Failed to open the symref store: {e:#}"
                    )))
                })
            })
            .await
    }
}

#[async_trait::async_trait]
impl SymrefStoreProvider for SymrefStoreFactory {
    async fn repo_store(
        &self,
        repo_id: RepositoryId,
    ) -> Result<Arc<dyn GitSymbolicRefs>, scs_errors::ServiceError> {
        let builder = self.shared_builder().await?;
        Ok(Arc::new(builder.clone().build(repo_id)))
    }
}

/// `enabled` is the batch-wide decision from `validate_default_branches`;
/// the JK must not be re-evaluated here.
async fn write_default_branch_symrefs(
    ctx: &CoreContext,
    symref_store_provider: &dyn SymrefStoreProvider,
    repo_ids_and_requests: &[(RepositoryId, thrift::RepoCreationRequest)],
    enabled: bool,
) -> Result<Vec<RepositoryId>, scs_errors::ServiceError> {
    if !enabled {
        return Ok(Vec::new());
    }
    let targets = repo_ids_and_requests
        .iter()
        .filter_map(|(repo_id, request)| {
            request
                .default_branch
                .clone()
                .map(|branch| (*repo_id, branch))
        })
        .collect::<Vec<_>>();
    if targets.is_empty() {
        return Ok(Vec::new());
    }

    let repo_ids = targets
        .iter()
        .map(|(repo_id, _branch)| *repo_id)
        .collect::<Vec<_>>();
    stream::iter(targets)
        .map(|(repo_id, branch)| async move {
            let store = symref_store_provider.repo_store(repo_id).await?;
            write_default_branch_symref(ctx, store.as_ref(), &branch).await
        })
        .buffer_unordered(10)
        .try_collect::<Vec<()>>()
        .await?;

    Ok(repo_ids)
}

async fn delete_head_symrefs(
    ctx: &CoreContext,
    symref_store_provider: &dyn SymrefStoreProvider,
    repo_ids: &[RepositoryId],
) -> Result<(), scs_errors::ServiceError> {
    stream::iter(repo_ids.iter().copied())
        .map(|repo_id| async move {
            let store = symref_store_provider.repo_store(repo_id).await?;
            store
                .delete_symrefs(ctx, vec![HEAD_SYMREF.to_string()])
                .await
                .map_err(|e| {
                    scs_errors::ServiceError::from(scs_errors::internal_error(format!(
                        "Failed to delete the HEAD symref for repo {repo_id}: {e:#}"
                    )))
                })
        })
        .buffer_unordered(10)
        .try_collect::<Vec<()>>()
        .await?;
    Ok(())
}

async fn cleanup_reserved_repos_after_failure(
    ctx: &CoreContext,
    git_source_of_truth_config: &dyn GitSourceOfTruthConfig,
    symref_store_provider: &dyn SymrefStoreProvider,
    symref_repo_ids: &[RepositoryId],
    params: &thrift::CreateReposParams,
) -> Result<(), scs_errors::ServiceError> {
    // Symref deletes go before the SoT deletes so a failure leaves the rows
    // Reserved and retryable; same retry shape as the poll-path cleanup.
    retry(
        |_| delete_head_symrefs(ctx, symref_store_provider, symref_repo_ids),
        Duration::from_millis(1_000),
    )
    .binary_exponential_backoff()
    .max_attempts(5)
    .await?;
    retry(
        |_| {
            delete_source_of_truth_for_reserved_repos(
                ctx.clone(),
                git_source_of_truth_config,
                params,
            )
        },
        Duration::from_millis(1_000),
    )
    .binary_exponential_backoff()
    .max_attempts(5)
    .await?;
    Ok(())
}

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
                (AUTH_SET, "coding_crewmates"),
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
        .commitChangeSpecificationV2(&request)
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
            "Hipster group: {hipster_group} is not a maintainer for acl: {acl_name}"
        ))
        .into());
    }
    // Ensure this oncall is point of contact for this ACL
    if acl.point_of_contact.id_data != oncall_name {
        return Err(scs_errors::invalid_request(format!(
            "Oncall: {oncall_name} is not a point of contact for acl: {acl_name}"
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

/// Build a Hipster ACL name for a repo, lowercased.
///
/// Hipster auto-lowercases ACL names on write (visible in `consumer.id_data`
/// from `hipstercli getrawacl`). Mononoke's per-permission push-time check
/// (e.g. the `BYPASS_ALL_HOOKS` pushvar check on `write_no_hooks`) is
/// case-sensitive against the ACL name stored on the repo, so without
/// normalization a repo whose name has uppercase characters ends up
/// configured with an ACL name that no Hipster entry matches.
///
/// Concrete failure mode: XF-APAC/dreamwright-v2 mirror sync, 2026-06-24
/// 16:44 UTC. The Mononoke repo was configured with
/// `custom_acl_name="repos/git/XF-APAC"` but the actual Hipster entry was
/// `repos/git/xf-apac`. Every `gitimport --bypass-all-hooks` push to it
/// failed with "needs … write_no_hooks action on repo ACL" because the
/// case-sensitive grant lookup missed. par-msl was unaffected only because
/// its org slug was already lowercase.
///
/// Lowercasing here keeps Mononoke's stored ACL name byte-equal to what
/// Hipster will return for the same logical ACL, for every tenant
/// regardless of how their org slug is cased on github.com.
fn make_full_acl_name_from_repo_name(repo_name: &str) -> String {
    format!("repos/git/{}", repo_name.to_lowercase())
}

fn make_top_level_acl_name_from_repo_name(repo_name: &str) -> String {
    // IMPORTANT: this hardcodes "repos/git/" because create_repos only supports GIT today.
    // Hg repos use "repos/hg/<name>" ACLs (e.g., "repos/hg/aosp"). When adding HG support
    // to create_repos, this function must branch on identity_scheme — see the
    // _IDENTITY_SUBDIR mapping in configerator/source/scm/mononoke/repos/generate_repo_index.py.
    // NOTE for future implementer: any logging added inside add_repo() must use debug! not info!
    // — info! in add_repo() breaks .t integration tests (project memory).
    //
    // Case normalization: see `make_full_acl_name_from_repo_name` docstring
    // for the rationale (XF-APAC mirror sync SEV, 2026-06-24).
    let (top_level, _rest) = repo_name.split_once('/').unwrap_or((repo_name, ""));
    format!("repos/git/{}", top_level.to_lowercase())
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

/// Outcome of reserving repo ids for a `create_repos` batch.
#[cfg(fbcode_build)]
#[derive(Debug)]
enum ReserveOutcome {
    /// Repos were freshly reserved; caller must prepare + land a mutation.
    Reserved(Vec<(RepositoryId, thrift::RepoCreationRequest)>),
    /// All requested repos were already `reserved` under a single in-flight
    /// mutation; caller should attach to it and return the token unchanged.
    AttachedToInflight { mutation_id: i64 },
}

#[cfg(fbcode_build)]
async fn reserve_repos_ids(
    ctx: CoreContext,
    git_source_of_truth_config: &dyn GitSourceOfTruthConfig,
    params: &thrift::CreateReposParams,
) -> Result<ReserveOutcome, scs_errors::ServiceError> {
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
            Ok(_) => Ok(ReserveOutcome::Reserved(repo_ids_and_requests)),
            Err(e) => {
                let error_trace = format!("{e:#}");
                // Match both SQLite ("UNIQUE constraint failed") and MySQL
                // ("Duplicate entry '...' for key 'repo_name_idx'") errors.
                let is_duplicate = (error_trace.contains("UNIQUE constraint failed")
                    && error_trace.contains("git_repositories_source_of_truth.repo_name"))
                    || (error_trace.contains("Duplicate entry")
                        && error_trace.contains("repo_name_idx"));
                if is_duplicate {
                    // Look up every requested repo's current row so we can both
                    // build human-readable `details` (today's behavior) and, when
                    // the attach knob is enabled, classify whether this is a
                    // duplicate request for a single in-flight mutation we can
                    // safely attach to.
                    let mut details = Vec::new();
                    let mut lookups = Vec::with_capacity(repo_ids_and_requests.len());
                    for (_id, request) in &repo_ids_and_requests {
                        let repo_name = RepositoryName(request.repo_name.clone());
                        let lookup = git_source_of_truth_config
                            .get_by_repo_name(&ctx, &repo_name, Staleness::MostRecent)
                            .await;
                        match &lookup {
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
                        lookups.push(lookup);
                    }

                    let attach_enabled = justknobs::eval(ATTACH_JK, None, None);

                    if attach_enabled {
                        // A lookup `Err` is a transient DB/query failure, NOT a
                        // client-side invalid request. Surface it as an internal
                        // error (mapped to `ServiceError::Internal`) so the retry
                        // loop in `create_repos_in_mononoke` retries it, instead
                        // of masking a retryable failure as `invalid_request`.
                        let lookup_errors = lookups
                            .iter()
                            .filter_map(|lookup| lookup.as_ref().err())
                            .map(|e| format!("{e:#}"))
                            .collect::<Vec<_>>();
                        if !lookup_errors.is_empty() {
                            return Err(scs_errors::internal_error(format!(
                                "Failed to look up reserved repos while classifying a duplicate \
                                 creation request: {}",
                                lookup_errors.join("; ")
                            ))
                            .into());
                        }

                        // Only attach when every requested repo resolved to a
                        // `Reserved` row stamped with the SAME mutation_id.
                        let all_reserved_entries = lookups
                            .iter()
                            .map(|lookup| match lookup {
                                Ok(Some(entry))
                                    if entry.source_of_truth == GitSourceOfTruth::Reserved =>
                                {
                                    entry.mutation_id
                                }
                                _ => None,
                            })
                            .collect::<Vec<_>>();

                        // All lookups are `Ok` here (errors returned above), so a
                        // non-reserved entry is a genuine split-brain / missing-row
                        // case, not a transient failure.
                        let any_non_reserved = lookups.iter().any(|lookup| {
                            !matches!(
                                lookup,
                                Ok(Some(entry)) if entry.source_of_truth == GitSourceOfTruth::Reserved
                            )
                        });

                        if any_non_reserved {
                            // At least one row is not `Reserved` (or lookup
                            // failed / returned None). If any row is present but
                            // in a non-reserved state, this is the split-brain
                            // case and `details` already carries the DANGER
                            // message. Fall through to the shared error below.
                            return Err(scs_errors::invalid_request(details.join("\n")).into());
                        }

                        // Every row is `Reserved`. Decide based on stamping.
                        if all_reserved_entries.iter().any(Option::is_none) {
                            return Err(scs_errors::invalid_request(format!(
                                "Repo creation is already in progress but not yet trackable \
                                 (a reserved row has no mutation_id stamped yet); retry shortly, \
                                 or delete the stale reserved row if the original attempt died.\n{}",
                                details.join("\n")
                            ))
                            .into());
                        }

                        let mutation_ids = all_reserved_entries
                            .iter()
                            .filter_map(|id| *id)
                            .collect::<std::collections::BTreeSet<_>>();
                        let mut ids = mutation_ids.iter();
                        match (ids.next(), ids.next()) {
                            (Some(mutation_id), None) => {
                                // Exactly one distinct in-flight mutation: attach.
                                let repo_names = repo_ids_and_requests
                                    .iter()
                                    .map(|(_id, request)| request.repo_name.as_str())
                                    .collect::<Vec<_>>()
                                    .join(",");
                                let mut scuba = ctx.scuba().clone();
                                scuba.add("action", "create_repos_attach");
                                scuba.add("attached_mutation_id", *mutation_id);
                                scuba.add("repo_names", repo_names);
                                scuba.log_with_msg(
                                    "create_repos attached to in-flight mutation",
                                    None,
                                );
                                return Ok(ReserveOutcome::AttachedToInflight {
                                    mutation_id: *mutation_id,
                                });
                            }
                            (None, _) => {
                                // Defensive: no mutation ids collected. Unreachable
                                // in practice — the all-Some guard above ensures
                                // every reserved repo has a stamped id; reachable
                                // only for an empty batch, which cannot hit the
                                // duplicate path.
                                return Err(scs_errors::invalid_request(format!(
                                    "No reserved repos to attach to.\n{}",
                                    details.join("\n")
                                ))
                                .into());
                            }
                            (Some(_), Some(_)) => {
                                // More than one distinct in-flight mutation.
                                return Err(scs_errors::invalid_request(format!(
                                    "Repo creation request is not idempotent: the reserved repos \
                                     span multiple in-flight mutations; resolve manually.\n{}",
                                    details.join("\n")
                                ))
                                .into());
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

/// Returns the tier list for a new repo as owned `String`s
/// (matches the `Vec<String>` type used by `RepoSpec::tiers`).
/// See `repo_spec_writer::tier_list_for_repo_spec` for substring-based
/// `aosp/` matching behavior.
fn tier_list_for_repo(repo_name: &str) -> Vec<String> {
    tier_list_for_repo_spec(repo_name)
        .into_iter()
        .map(str::to_string)
        .collect()
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

// RepoSpec path helpers, Python-literal formatters, RepoIndexEntry, and
// append_to_repo_index live in `repo_spec_writer` so the Phase 6 RepoSpec
// migrator can share the same byte-equivalent implementation. See
// `eden/mononoke/tools/repo_spec_writer/src/lib.rs`.

fn make_repo_spec(
    (repo_id, request): &(RepositoryId, thrift::RepoCreationRequest),
    default_repo_config: Option<RawRepoConfig>,
) -> Result<RepoSpec, scs_errors::ServiceError> {
    Ok(RepoSpec {
        repo_id: repo_id.id(),
        repo_name: request.repo_name.clone(),
        hipster_acl: if request.custom_acl.is_some() {
            make_full_acl_name_from_repo_name(&request.repo_name)
        } else {
            make_top_level_acl_name_from_repo_name(&request.repo_name)
        },
        enabled: true,
        readonly: false,
        default_commit_identity_scheme: RawCommitIdentityScheme::GIT,
        enable_git_bundle_uri: None,
        tiers: tier_list_for_repo(&request.repo_name),
        t_shirt_size: to_repo_spec_tshirt_size(request.size_bucket)?,
        sharding_regions: ShardingRegions::BGM_ONLY_REGIONS,
        repo_config: default_repo_config,
        tier_overrides: None,
        ..Default::default()
    })
}

async fn prepare_repo_configs_mutation_nowait(
    ctx: CoreContext,
    repos_ids_and_requests: Vec<(RepositoryId, thrift::RepoCreationRequest)>,
    configs: &MononokeConfigs,
) -> Result<i64, scs_errors::ServiceError> {
    let configo_client = ConfigoClient::with_client(
        ctx.fb,
        make_ConfigoService_srclient!(ctx.fb)
            .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?,
    );
    let mut txn = configo_client.managed_transaction();

    // Load the default git repo config template once before the loop.
    let config_store = configs.config_store().ok_or_else(|| {
        scs_errors::internal_error("No config store available for loading default repo config")
    })?;
    let default_repo_config =
        configerator_repo_config_handle(DEFAULT_GIT_REPO_CONFIG_PATH, config_store)
            .map_err(|e| {
                scs_errors::internal_error(format!("Failed to load default git repo config: {e:#}"))
            })?
            .get();

    // Create individual repo config files
    for (repo_id, request) in &repos_ids_and_requests {
        let repo_spec = make_repo_spec(
            &(*repo_id, request.clone()),
            Some((*default_repo_config).clone()),
        )?;
        // create_repos only ever creates git repos (see RepoSpecDir docs).
        let file_path = make_repo_spec_file_path(&request.repo_name, RepoSpecDir::Git);
        txn.set_thrift_object(
            repo_spec,
            file_path,
            REPO_SPEC_THRIFT_TYPE.to_string(),
            REPO_SPEC_THRIFT_PATH.to_string(),
            None,
        );
    }

    // Update repo_index.cinc atomically in the same transaction.
    let index_path = "source/scm/mononoke/repos/repo_index.cinc".to_string();

    // Read current content — pins CAS version. Must drop handle before set_file.
    let index_str = {
        let handle = txn.get_file(index_path.clone()).await.map_err(|e| {
            scs_errors::internal_error(format!("Failed to read repo_index.cinc: {e:#}"))
        })?;
        String::from_utf8(handle.clone()).map_err(|e| {
            scs_errors::internal_error(format!("repo_index.cinc is not valid UTF-8: {e:#}"))
        })?
    }; // handle dropped — txn no longer borrowed

    // Build entries for all new repos
    let new_entries: Vec<_> = repos_ids_and_requests
        .iter()
        .map(|(repo_id, request)| {
            let config_path = make_repo_spec_config_path(&request.repo_name, RepoSpecDir::Git);
            let t_shirt_size = to_repo_spec_tshirt_size(request.size_bucket)?;
            Ok((
                request.repo_name.clone(),
                RepoIndexEntry {
                    config_path,
                    repo_id: repo_id.id(),
                    tiers: tier_list_for_repo_spec(&request.repo_name),
                    is_deep_sharded: true,
                    t_shirt_size,
                    hipster_acl: if request.custom_acl.is_some() {
                        make_full_acl_name_from_repo_name(&request.repo_name)
                    } else {
                        make_top_level_acl_name_from_repo_name(&request.repo_name)
                    },
                    enable_git_bundle_uri: None,
                },
            ))
        })
        .collect::<Result<_, scs_errors::ServiceError>>()?;

    let updated_index = append_to_repo_index(&index_str, &new_entries).map_err(|e| {
        scs_errors::internal_error(format!("Failed to update repo_index.cinc: {e:#}"))
    })?;
    txn.set_file(index_path, updated_index.into_bytes());

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
    let expected = params.repos.len() as u64;
    let stamped = git_source_of_truth_config
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
    // Every reserved row must still exist when it is stamped. A shortfall
    // means rows were lost (e.g. deleted out-of-band) after reservation:
    // fail the creation loudly before it proceeds toward landing, instead
    // of landing repos with no source-of-truth row.
    // MySQL affected_rows counts CHANGED rows; the confirm-read distinguishes a lost-ack re-stamp from genuine row loss.
    if stamped != expected
        && !all_requested_repos_stamped(&ctx, git_source_of_truth_config, params, mutation_id)
            .await?
    {
        return Err(scs_errors::internal_error(format!(
            "stamping mutation_id {mutation_id} updated {stamped} reserved row(s) but {expected} \
             were reserved; refusing to proceed, reconcile the source-of-truth table"
        ))
        .into());
    }
    Ok(())
}

async fn all_requested_repos_stamped(
    ctx: &CoreContext,
    git_source_of_truth_config: &dyn GitSourceOfTruthConfig,
    params: &thrift::CreateReposParams,
    mutation_id: i64,
) -> Result<bool, scs_errors::ServiceError> {
    // A row can flip Reserved -> Mononoke while the stamp is still retrying:
    // once the stamp has committed (even with its ack lost), a concurrent
    // duplicate request can attach to this mutation_id, and its client's poll
    // can land the mutation and flip the rows before the retry loop is done.
    // A flip proves the stamp committed, so rows in either state count.
    let stamped_names: HashSet<String> = git_source_of_truth_config
        .get_reserved_by_mutation_id(ctx, mutation_id)
        .await
        .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?
        .into_iter()
        .chain(
            git_source_of_truth_config
                .get_redirected_to_mononoke_by_mutation_id(ctx, mutation_id)
                .await
                .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?,
        )
        .map(|entry| entry.repo_name.0)
        .collect();
    Ok(params
        .repos
        .iter()
        .all(|request| stamped_names.contains(&request.repo_name)))
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
    symref_store_factory: SymrefStoreFactory,
    params: &thrift::CreateReposParams,
    configs: &MononokeConfigs,
    write_default_branch_symrefs_enabled: bool,
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

    let (outcome, _attempts) = retry(
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

    let repo_ids_and_requests = match outcome {
        ReserveOutcome::Reserved(repos) => repos,
        // A concurrent/duplicate request already reserved these repos under a
        // single in-flight mutation. Attach to it and return its token so the
        // caller can poll it to completion.
        ReserveOutcome::AttachedToInflight { mutation_id } => return Ok(Some(mutation_id)),
    };

    // Written at reservation time: the poll may run on another host that only has the mutation_id.
    let written_symref_repo_ids = match write_default_branch_symrefs(
        &ctx,
        &symref_store_factory,
        &repo_ids_and_requests,
        write_default_branch_symrefs_enabled,
    )
    .await
    {
        Ok(written) => written,
        Err(write_err) => {
            let attempted = repo_ids_and_requests
                .iter()
                .filter(|(_repo_id, request)| request.default_branch.is_some())
                .map(|(repo_id, _request)| *repo_id)
                .collect::<Vec<_>>();
            if let Err(cleanup_err) = cleanup_reserved_repos_after_failure(
                &ctx,
                git_source_of_truth_config.as_ref(),
                &symref_store_factory,
                &attempted,
                params,
            )
            .await
            {
                warn!(
                    "cleanup after create_repos symref write failure also failed; original error: {:?}",
                    write_err
                );
                return Err(cleanup_err);
            }
            return Err(write_err);
        }
    };

    // We have reserved the repo ids. Now it's time to actually create the repos, safe in the
    // knowledge that no-one will compete with us
    match prepare_repo_configs_mutation_nowait(ctx.clone(), repo_ids_and_requests, configs).await {
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

            // Clone necessary data for the spawned task
            let poll_ctx = ctx.clone();
            let git_sot_config = git_source_of_truth_config.clone();
            let poll_symref_store_factory = symref_store_factory.clone();

            mononoke::spawn_task({
                async move {
                    // Poll interval - start with 5 seconds
                    let poll_interval = Duration::from_secs(5);

                    loop {
                        match poll_mutation_id(
                            poll_ctx.clone(),
                            git_sot_config.as_ref(),
                            &poll_symref_store_factory,
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
                                    info!("mutation in progress for mutation_id {}", mutation_id);
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
            Ok(Some(mutation_id))
        }
        Err(e) => {
            // We failed to land the mutation, so it is safe to "release the lock" on these repo
            // ids and names, which will allow a future attempt to succeed.
            if let Err(cleanup_err) = cleanup_reserved_repos_after_failure(
                &ctx,
                git_source_of_truth_config.as_ref(),
                &symref_store_factory,
                &written_symref_repo_ids,
                params,
            )
            .await
            {
                warn!(
                    "cleanup after create_repos mutation prepare failure also failed; original error: {:?}",
                    e
                );
                return Err(cleanup_err);
            }
            Err(e)
        }
    }
}

#[cfg(not(fbcode_build))]
async fn create_repos_in_mononoke(
    _ctx: CoreContext,
    _git_source_of_truth_config: Arc<dyn GitSourceOfTruthConfig>,
    _symref_store_factory: SymrefStoreFactory,
    _params: &thrift::CreateReposParams,
    _configs: &MononokeConfigs,
    _write_default_branch_symrefs_enabled: bool,
) -> Result<Option<i64>, scs_errors::ServiceError> {
    println!("No access to configo in oss build");
    Ok(None)
}

impl SourceControlServiceImpl {
    fn symref_store_factory(&self) -> SymrefStoreFactory {
        SymrefStoreFactory {
            fb: self.fb,
            configs: self.configs.clone(),
            mysql_options: self.mysql_options.clone(),
            builder: Arc::new(OnceCell::new()),
        }
    }

    pub(crate) async fn create_repos(
        &self,
        ctx: CoreContext,
        params: thrift::CreateReposParams,
    ) -> Result<thrift::CreateReposToken, scs_errors::ServiceError> {
        // Must precede `ensure_acls_allow_repo_creation`, which fans out one
        // Hipster/Oncall RPC per requested repo with an unbounded `try_join_all`:
        // a cap applied afterwards would already have paid the cost it bounds.
        //
        // Advisory unless the enforce knob is on. `create_repos` has always
        // accepted any batch, including empty ones, and both the bulk-import
        // pipeline and `scmadmin repo tasks` are unattended, so a rejection
        // that arrives without a measured log-only phase fails a partner run on
        // a number we guessed (`config_rollout_safety.md`, S578742).
        let count = params.repos.len();
        let tier = batch_size_tier_for_caller(&ctx, self.acl_provider.as_ref()).await;
        if let Err(error) = check_batch_size(count, tier.max_batch_size()) {
            if justknobs::eval(ENFORCE_BATCH_SIZE_JK, None, Some(tier.name())) {
                return Err(error);
            }
            warn!(
                count,
                max_batch_size = tier.max_batch_size(),
                tier = tier.name(),
                "create_repos batch is outside the cap for this tier; admitting it because \
                 enforcement is off"
            );
        }

        let write_default_branch_symrefs_enabled = validate_default_branches(&params.repos)?;

        ensure_acls_allow_repo_creation(ctx.clone(), &params.repos, self.acl_provider.as_ref())
            .await?;
        update_repos_acls(ctx.clone(), &params).await?;
        let mutation_id = create_repos_in_mononoke(
            ctx,
            self.git_source_of_truth_config.clone(),
            self.symref_store_factory(),
            &params,
            &self.configs,
            write_default_branch_symrefs_enabled,
        )
        .await?;

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
        let symref_store_factory = self.symref_store_factory();
        let mutation_state;
        let status = match poll_mutation_id(
            ctx,
            self.git_source_of_truth_config.as_ref(),
            &symref_store_factory,
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
                            "Unexpected Configo mutation state: {state}"
                        ))
                        .into());
                    }
                }
            }
            Err(err) => return Err(err),
        };

        let message = Some(format!("Mutation state: {mutation_state}"));
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
    symref_store_provider: &dyn SymrefStoreProvider,
    mutation_id: i64,
) -> Result<MutationState, scs_errors::ServiceError> {
    match poll_mutation_id_impl(
        ctx.clone(),
        git_source_of_truth_config,
        symref_store_provider,
        mutation_id,
        0,
    )
    .await
    {
        Ok(state) => Ok(state),
        Err(e) => match e {
            scs_errors::ServiceError::Poll(_) => {
                poll_mutation_id_impl(
                    ctx.clone(),
                    git_source_of_truth_config,
                    symref_store_provider,
                    mutation_id,
                    1,
                )
                .await
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
                    "Configo mutation error: {error_message}"
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
    symref_store_provider: &dyn SymrefStoreProvider,
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
            cleanup_repos(
                ctx,
                git_source_of_truth_config,
                symref_store_provider,
                mutation_id,
            )
            .await?;
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
    symref_store_provider: &dyn SymrefStoreProvider,
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
        cleanup_repos(
            ctx.clone(),
            git_source_of_truth_config,
            symref_store_provider,
            mutation_id,
        )
        .await?;
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
        cleanup_repos(
            ctx.clone(),
            git_source_of_truth_config,
            symref_store_provider,
            mutation_id,
        )
        .await?;
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
        symref_store_provider,
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

async fn delete_head_symrefs_for_mutation_id(
    ctx: CoreContext,
    git_source_of_truth_config: &dyn GitSourceOfTruthConfig,
    symref_store_provider: &dyn SymrefStoreProvider,
    mutation_id: i64,
) -> Result<(), scs_errors::ServiceError> {
    let reserved_repo_ids = git_source_of_truth_config
        .get_reserved_by_mutation_id(&ctx, mutation_id)
        .await
        .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?
        .into_iter()
        .map(|entry| entry.repo_id)
        .collect::<Vec<_>>();
    delete_head_symrefs(&ctx, symref_store_provider, &reserved_repo_ids).await
}

/// Two concurrent pollers can both run this; a stalled one could delete a fresh HEAD row
/// after id reuse plus re-reserve+rewrite. Not atomically fixable across the two DBs; accepted.
async fn cleanup_repos(
    ctx: CoreContext,
    git_source_of_truth_config: &dyn GitSourceOfTruthConfig,
    symref_store_provider: &dyn SymrefStoreProvider,
    mutation_id: i64,
) -> Result<(), scs_errors::ServiceError> {
    // Symref deletes go before the SoT deletes so a failure leaves the rows Reserved and retryable.
    let (_result, _attempts) = retry(
        |_| {
            delete_head_symrefs_for_mutation_id(
                ctx.clone(),
                git_source_of_truth_config,
                symref_store_provider,
                mutation_id,
            )
        },
        Duration::from_millis(1_000),
    )
    .binary_exponential_backoff()
    .max_attempts(5)
    .await?;
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
    use fbinit::FacebookInit;
    use git_symbolic_refs::SqlGitSymbolicRefsBuilder;
    use mononoke_macros::mononoke;
    use sql_construct::SqlConstruct;

    use super::*;

    #[mononoke::test]
    fn test_make_repo_spec_file_path_simple_name() {
        // Test asserts the git-only path. When adding HG support to create_repos, add a
        // parallel test for repos/hg/ paths.
        let path = make_repo_spec_file_path("my-repo", RepoSpecDir::Git);
        assert!(
            path.starts_with("source/scm/mononoke/repos/git/"),
            "Path should start with RepoSpec base path: {path}"
        );
        assert!(
            path.ends_with("/my-repo.cconf"),
            "Path should end with /repo-name.cconf: {path}"
        );
    }

    #[mononoke::test]
    fn test_make_repo_spec_file_path_slash_in_name() {
        let path = make_repo_spec_file_path("org/project/repo", RepoSpecDir::Git);
        assert!(
            path.ends_with("/org_project_repo.cconf"),
            "Slashes should be replaced with underscores: {path}"
        );
    }

    #[mononoke::test]
    fn test_make_repo_spec_file_path_no_collision_slash_vs_underscore() {
        let path1 = make_repo_spec_file_path("org/repo", RepoSpecDir::Git);
        let path2 = make_repo_spec_file_path("org_repo", RepoSpecDir::Git);
        assert_ne!(
            path1, path2,
            "Repos differing only in '/' vs '_' must produce different paths"
        );
    }

    #[mononoke::test]
    fn test_make_repo_spec_file_path_deterministic() {
        let path1 = make_repo_spec_file_path("test/repo", RepoSpecDir::Git);
        let path2 = make_repo_spec_file_path("test/repo", RepoSpecDir::Git);
        assert_eq!(path1, path2, "Hash-based path should be deterministic");
    }

    #[mononoke::test]
    fn test_make_repo_spec_file_path_different_repos_may_differ() {
        let path1 = make_repo_spec_file_path("repo-alpha", RepoSpecDir::Git);
        let path2 = make_repo_spec_file_path("repo-beta", RepoSpecDir::Git);
        assert_ne!(
            path1, path2,
            "Different repos should produce different paths"
        );
    }

    #[mononoke::test]
    fn test_make_top_level_acl_name_lowercases_uppercase_org() {
        // Regression for the XF-APAC mirror sync failure on 2026-06-24:
        // before this fix, the uppercase repo name flowed through verbatim
        // to the ACL name on the Mononoke repo config, producing a
        // case-mismatch with the lowercased Hipster ACL.
        assert_eq!(
            make_top_level_acl_name_from_repo_name("XF-APAC/dreamwright-v2"),
            "repos/git/xf-apac",
        );
    }

    #[mononoke::test]
    fn test_make_top_level_acl_name_preserves_already_lowercase_org() {
        // Existing tenants (par-msl) must keep producing the byte-equal
        // ACL name they had before this fix — otherwise their repo
        // configs would point at a different name than the live Hipster
        // entries on the next config rewrite.
        assert_eq!(
            make_top_level_acl_name_from_repo_name("par-msl/risk-test"),
            "repos/git/par-msl",
        );
    }

    #[mononoke::test]
    fn test_make_top_level_acl_name_no_slash() {
        // Defensive: repo name without a slash falls back to using the
        // whole name as the top-level (matches the pre-fix behavior
        // shape), still lowercased.
        assert_eq!(
            make_top_level_acl_name_from_repo_name("Single-Segment"),
            "repos/git/single-segment",
        );
    }

    #[mononoke::test]
    fn test_make_full_acl_name_lowercases() {
        // Custom-ACL path (callers with `custom_acl.is_some()`) also goes
        // through Hipster's lowercasing, so the full ACL name must be
        // lowercased end-to-end.
        assert_eq!(
            make_full_acl_name_from_repo_name("XF-APAC/Dreamwright-V2"),
            "repos/git/xf-apac/dreamwright-v2",
        );
        assert_eq!(
            make_full_acl_name_from_repo_name("par-msl/risk-test"),
            "repos/git/par-msl/risk-test",
        );
    }

    #[cfg(fbcode_build)]
    #[mononoke::test]
    fn test_initial_acl_grants_include_coding_crewmates_read() {
        // Every newly-created per-repo Git ACL must grant read to
        // AUTH_SET:coding_crewmates so all Meta engineers can clone the
        // repo. Removing this grant would silently regress the eliminate
        // -per-repo-onboarding-friction commitment made after the
        // provide_gitimport_read_access.sh backfill; grep for that script
        // name before deleting this assertion.
        let grants = initial_acl_grants("some_hipster_group");
        let read = grants
            .iter()
            .find(|g| g.action == "read")
            .expect("initial_acl_grants must contain a read action");
        let has_coding_crewmates = read
            .entry_changes
            .iter()
            .any(|e| e.entry.id_type == AUTH_SET && e.entry.id_data == "coding_crewmates");
        assert!(
            has_coding_crewmates,
            "initial_acl_grants read action must grant AUTH_SET:coding_crewmates",
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

        let spec =
            make_repo_spec(&(repo_id, request), None).expect("make_repo_spec should succeed");

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
        assert_eq!(
            spec.tiers,
            vec!["gitimport", "gitimport_content", "scs", "backfill_worker"]
        );
        assert!(
            spec.repo_config.is_none(),
            "New repos should have no custom config"
        );
        assert!(
            spec.tier_overrides.is_none(),
            "New repos should have no tier overrides"
        );
        assert_eq!(
            spec.hipster_acl, "repos/git/org",
            "hipster_acl should be the top-level namespace ACL, not the full repo name"
        );
    }

    #[mononoke::test]
    fn test_make_repo_spec_uses_top_level_acl_for_aosp_repo() {
        let repo_id = RepositoryId::new(18279);
        let request = thrift::RepoCreationRequest {
            repo_name: "aosp/platform/vendor/meta/prebuilts/assets".to_string(),
            size_bucket: RepoSizeBucket::SMALL,
            ..Default::default()
        };

        let spec =
            make_repo_spec(&(repo_id, request), None).expect("make_repo_spec should succeed");

        assert_eq!(
            spec.hipster_acl, "repos/git/aosp",
            "AOSP repos must use the top-level `repos/git/aosp` ACL, not a non-existent full-path ACL"
        );
    }

    #[mononoke::test]
    fn test_make_repo_spec_uses_full_name_when_no_slash() {
        let repo_id = RepositoryId::new(99999);
        let request = thrift::RepoCreationRequest {
            repo_name: "simple-repo".to_string(),
            size_bucket: RepoSizeBucket::SMALL,
            ..Default::default()
        };

        let spec =
            make_repo_spec(&(repo_id, request), None).expect("make_repo_spec should succeed");

        assert_eq!(
            spec.hipster_acl, "repos/git/simple-repo",
            "Repos without `/` should use the full name as the ACL"
        );
    }

    #[mononoke::test]
    fn test_tier_list_for_repo_spec_aosp_prefix_adds_multi_repo_land() {
        assert_eq!(
            tier_list_for_repo_spec("aosp/platform/vendor/foo"),
            vec![
                "gitimport",
                "gitimport_content",
                "scs",
                "backfill_worker",
                "aosp_multi_repo_land",
            ],
            "aosp/* repos must include aosp_multi_repo_land tier"
        );
    }

    #[mononoke::test]
    fn test_tier_list_for_repo_spec_nested_aosp_adds_multi_repo_land() {
        // Substring match: repos with `aosp/` deeper in the path (e.g. the
        // Oculus AOSP fork) must also be on the aosp_multi_repo_land tier.
        assert_eq!(
            tier_list_for_repo_spec("oculus/aosp/vendor/oculus"),
            vec![
                "gitimport",
                "gitimport_content",
                "scs",
                "backfill_worker",
                "aosp_multi_repo_land",
            ],
            "repos containing aosp/ as a substring must include aosp_multi_repo_land tier"
        );
    }

    #[mononoke::test]
    fn test_tier_list_for_repo_spec_non_aosp_excluded_from_multi_repo_land() {
        assert_eq!(
            tier_list_for_repo_spec("manus/foo"),
            vec!["gitimport", "gitimport_content", "scs", "backfill_worker"],
            "non-aosp repos must NOT include aosp_multi_repo_land tier"
        );
        assert_eq!(
            tier_list_for_repo_spec("simple-repo"),
            vec!["gitimport", "gitimport_content", "scs", "backfill_worker"],
            "simple repos must NOT include aosp_multi_repo_land tier"
        );
        // Boundary: a repo literally named "aosp" (no slash) does NOT contain `aosp/`.
        assert_eq!(
            tier_list_for_repo_spec("aosp"),
            vec!["gitimport", "gitimport_content", "scs", "backfill_worker"],
            "literal name 'aosp' (no trailing /) must NOT match the aosp/ substring"
        );
        // Boundary: confusingly-named prefix that shares "aosp" but isn't `aosp/`.
        assert_eq!(
            tier_list_for_repo_spec("aosp_extras/foo"),
            vec!["gitimport", "gitimport_content", "scs", "backfill_worker"],
            "aosp_extras/* must NOT match the aosp/ substring"
        );
    }

    #[mononoke::test]
    fn test_tier_list_for_repo_string_variant_matches_repo_spec() {
        // The String variant must mirror the &'static str variant exactly.
        let s_aosp = tier_list_for_repo("aosp/platform/vendor/foo");
        let r_aosp = tier_list_for_repo_spec("aosp/platform/vendor/foo");
        assert_eq!(
            s_aosp,
            r_aosp.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        );

        let s_other = tier_list_for_repo("org/repo");
        let r_other = tier_list_for_repo_spec("org/repo");
        assert_eq!(
            s_other,
            r_other.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        );
    }

    #[mononoke::test]
    fn test_make_repo_spec_aosp_repo_includes_multi_repo_land_tier() {
        let repo_id = RepositoryId::new(18279);
        let request = thrift::RepoCreationRequest {
            repo_name: "aosp/platform/vendor/meta/prebuilts/assets".to_string(),
            size_bucket: RepoSizeBucket::SMALL,
            ..Default::default()
        };

        let spec =
            make_repo_spec(&(repo_id, request), None).expect("make_repo_spec should succeed");

        assert_eq!(
            spec.tiers,
            vec![
                "gitimport".to_string(),
                "gitimport_content".to_string(),
                "scs".to_string(),
                "backfill_worker".to_string(),
                "aosp_multi_repo_land".to_string(),
            ],
            "AOSP repos must be added to aosp_multi_repo_land tier so multi_repo_land_service can serve them"
        );
    }

    #[mononoke::test]
    fn test_default_branch_validation() {
        for invalid in [
            "",
            "HEAD",
            "head",
            "Head",
            "hEaD",
            "refs/heads/main",
            "refs/tags/v1.0",
        ] {
            let err = validate_default_branch(invalid)
                .expect_err(&format!("'{invalid}' must be rejected as a default_branch"));
            assert!(
                matches!(err, scs_errors::ServiceError::Request(_)),
                "'{invalid}' must be rejected as an invalid request, got: {err:?}"
            );
        }
        for valid in ["main", "release/1.0"] {
            assert_eq!(
                validate_default_branch(valid)
                    .unwrap_or_else(|err| panic!("'{valid}' must be accepted, got: {err:?}")),
                valid,
            );
        }
    }

    #[mononoke::fbinit_test]
    async fn test_write_default_branch_symref_writes_head_row(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let store = SqlGitSymbolicRefsBuilder::with_sqlite_in_memory()?.build(RepositoryId::new(1));

        write_default_branch_symref(&ctx, &store, "main")
            .await
            .expect("writing the HEAD symref should succeed");

        let entry = store
            .get_ref_by_symref(&ctx, "HEAD".to_string())
            .await?
            .expect("a HEAD symref row should exist after the write");
        assert_eq!(entry.ref_name, "main");
        assert_eq!(entry.ref_name_with_type(), "refs/heads/main");
        Ok(())
    }

    #[mononoke::test]
    fn test_make_repo_spec_uses_full_acl_when_custom_acl_set() {
        let repo_id = RepositoryId::new(55555);
        let request = thrift::RepoCreationRequest {
            repo_name: "fairinternal/occhi".to_string(),
            size_bucket: RepoSizeBucket::SMALL,
            custom_acl: Some(thrift::CustomAclParams {
                hipster_group: "oncall_onevision".to_string(),
                ..Default::default()
            }),
            ..Default::default()
        };

        let spec =
            make_repo_spec(&(repo_id, request), None).expect("make_repo_spec should succeed");

        assert_eq!(
            spec.hipster_acl, "repos/git/fairinternal/occhi",
            "Repos with custom_acl should use the full-path ACL"
        );
    }
}

#[cfg(all(fbcode_build, test))]
mod attach_tests {
    use std::collections::HashMap;

    use fbinit::FacebookInit;
    use futures::FutureExt;
    use git_source_of_truth::GitSourceOfTruth;
    use git_source_of_truth::RepositoryName;
    use git_source_of_truth::SqlGitSourceOfTruthConfigBuilder;
    use justknobs::test_helpers::JustKnobsInMemory;
    use justknobs::test_helpers::KnobVal;
    use justknobs::test_helpers::with_just_knobs_async;
    use mononoke_macros::mononoke;
    use sql_construct::SqlConstruct;

    use super::*;

    fn params_for(names: &[&str]) -> thrift::CreateReposParams {
        thrift::CreateReposParams {
            repos: names
                .iter()
                .map(|n| thrift::RepoCreationRequest {
                    repo_name: (*n).to_string(),
                    scm_type: thrift::RepoScmType::GIT,
                    size_bucket: thrift::RepoSizeBucket::SMALL,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    #[mononoke::fbinit_test]
    async fn stamp_count_mismatch_fails_creation(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let config = SqlGitSourceOfTruthConfigBuilder::with_sqlite_in_memory()?.build();
        config
            .insert_repos(
                &ctx,
                &[(
                    RepositoryId::new(1),
                    RepositoryName("repo/a".to_string()),
                    GitSourceOfTruth::Reserved,
                )],
            )
            .await?;

        // Only one of the two expected reserved rows exists: the stamp must
        // fail the creation rather than proceed with a lost row.
        let params = params_for(&["repo/a", "repo/b"]);
        let result = update_mutation_id_by_repo_names_for_reserved_repos(
            ctx.clone(),
            &config,
            &params,
            4242,
        )
        .await;
        assert!(result.is_err(), "missing reserved row must fail the stamp");

        // With every expected row reserved, the stamp succeeds.
        config
            .insert_repos(
                &ctx,
                &[(
                    RepositoryId::new(2),
                    RepositoryName("repo/b".to_string()),
                    GitSourceOfTruth::Reserved,
                )],
            )
            .await?;
        update_mutation_id_by_repo_names_for_reserved_repos(ctx.clone(), &config, &params, 4243)
            .await
            .map_err(|e| anyhow::anyhow!("stamp should succeed: {e:?}"))?;

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn lost_ack_restamp_confirm_read(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let config = SqlGitSourceOfTruthConfigBuilder::with_sqlite_in_memory()?.build();
        config
            .insert_repos(
                &ctx,
                &[
                    (
                        RepositoryId::new(1),
                        RepositoryName("repo/a".to_string()),
                        GitSourceOfTruth::Reserved,
                    ),
                    (
                        RepositoryId::new(2),
                        RepositoryName("repo/b".to_string()),
                        GitSourceOfTruth::Reserved,
                    ),
                ],
            )
            .await?;
        // Attempt 1 committed the stamp but its ack was lost; the retry's
        // UPDATE reports 0 changed rows on MySQL. Sqlite counts matched rows,
        // so exercise the confirm-read directly against the stamped state.
        config
            .update_mutation_id_by_repo_names_for_reserved_repos(
                &ctx,
                &[
                    RepositoryName("repo/a".to_string()),
                    RepositoryName("repo/b".to_string()),
                ],
                4242,
            )
            .await?;

        let params = params_for(&["repo/a", "repo/b"]);
        assert!(
            all_requested_repos_stamped(&ctx, &config, &params, 4242)
                .await
                .map_err(|e| anyhow::anyhow!("confirm-read should succeed: {e:?}"))?,
            "every requested repo carries the stamp: the re-stamp must be accepted",
        );

        // Genuine loss: a requested repo with no stamped row must still fail.
        let params_missing = params_for(&["repo/a", "repo/b", "repo/c"]);
        assert!(
            !all_requested_repos_stamped(&ctx, &config, &params_missing, 4242)
                .await
                .map_err(|e| anyhow::anyhow!("confirm-read should succeed: {e:?}"))?,
            "a repo missing from the stamped set must not be accepted",
        );

        // Rows stamped under a different mutation are not ours.
        assert!(
            !all_requested_repos_stamped(&ctx, &config, &params, 9999)
                .await
                .map_err(|e| anyhow::anyhow!("confirm-read should succeed: {e:?}"))?,
            "rows stamped with another mutation_id must not be accepted",
        );

        // A stamped row that a concurrent attacher's poll already flipped to
        // Mononoke still proves the stamp committed: the re-stamp must be
        // accepted, not reported as row loss.
        config
            .update_source_of_truth_by_repo_names(
                &ctx,
                GitSourceOfTruth::Mononoke,
                &[RepositoryName("repo/b".to_string())],
            )
            .await?;
        assert!(
            all_requested_repos_stamped(&ctx, &config, &params, 4242)
                .await
                .map_err(|e| anyhow::anyhow!("confirm-read should succeed: {e:?}"))?,
            "a row flipped to Mononoke under our mutation_id still counts as stamped",
        );
        assert!(
            !all_requested_repos_stamped(&ctx, &config, &params, 9999)
                .await
                .map_err(|e| anyhow::anyhow!("confirm-read should succeed: {e:?}"))?,
            "flipped rows under another mutation_id must not be accepted",
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn attach_happy_path(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let config = SqlGitSourceOfTruthConfigBuilder::with_sqlite_in_memory()?.build();
        config
            .insert_repos(
                &ctx,
                &[(
                    RepositoryId::new(1),
                    RepositoryName("repo/a".to_string()),
                    GitSourceOfTruth::Reserved,
                )],
            )
            .await?;
        config
            .update_mutation_id_by_repo_names_for_reserved_repos(
                &ctx,
                &[RepositoryName("repo/a".to_string())],
                4242,
            )
            .await?;

        let params = params_for(&["repo/a"]);
        with_just_knobs_async(
            JustKnobsInMemory::new(HashMap::from([(
                ATTACH_JK.to_string(),
                KnobVal::Bool(true),
            )])),
            async {
                let outcome = reserve_repos_ids(ctx.clone(), &config, &params)
                    .await
                    .expect("reserve_repos_ids should succeed and attach");
                match outcome {
                    ReserveOutcome::AttachedToInflight { mutation_id } => {
                        assert_eq!(mutation_id, 4242);
                    }
                    ReserveOutcome::Reserved(_) => {
                        panic!("expected AttachedToInflight, got Reserved")
                    }
                }
                anyhow::Ok(())
            }
            .boxed(),
        )
        .await?;
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn null_mutation_window(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let config = SqlGitSourceOfTruthConfigBuilder::with_sqlite_in_memory()?.build();
        config
            .insert_repos(
                &ctx,
                &[(
                    RepositoryId::new(1),
                    RepositoryName("repo/a".to_string()),
                    GitSourceOfTruth::Reserved,
                )],
            )
            .await?;

        let params = params_for(&["repo/a"]);
        with_just_knobs_async(
            JustKnobsInMemory::new(HashMap::from([(
                ATTACH_JK.to_string(),
                KnobVal::Bool(true),
            )])),
            async {
                let err = reserve_repos_ids(ctx.clone(), &config, &params)
                    .await
                    .expect_err("expected an error for a null-mutation reserved repo");
                match &err {
                    scs_errors::ServiceError::Request(req) => {
                        assert!(
                            format!("{req:?}").contains("in progress"),
                            "message should mention 'in progress', got: {req:?}"
                        );
                    }
                    other => panic!("expected Request error, got: {other:?}"),
                }
                anyhow::Ok(())
            }
            .boxed(),
        )
        .await?;
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn split_brain_guard(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let config = SqlGitSourceOfTruthConfigBuilder::with_sqlite_in_memory()?.build();
        // Insert reserved, stamp a mutation id, then flip it to Mononoke.
        config
            .insert_repos(
                &ctx,
                &[(
                    RepositoryId::new(1),
                    RepositoryName("repo/a".to_string()),
                    GitSourceOfTruth::Reserved,
                )],
            )
            .await?;
        config
            .update_mutation_id_by_repo_names_for_reserved_repos(
                &ctx,
                &[RepositoryName("repo/a".to_string())],
                7,
            )
            .await?;
        config
            .update_source_of_truth_by_mutation_id(&ctx, GitSourceOfTruth::Mononoke, 7)
            .await?;

        let params = params_for(&["repo/a"]);
        with_just_knobs_async(
            JustKnobsInMemory::new(HashMap::from([(
                ATTACH_JK.to_string(),
                KnobVal::Bool(true),
            )])),
            async {
                let err = reserve_repos_ids(ctx.clone(), &config, &params)
                    .await
                    .expect_err("expected an error for a non-reserved (mononoke) repo");
                match &err {
                    scs_errors::ServiceError::Request(req) => {
                        assert!(
                            format!("{req:?}").contains("DANGER"),
                            "message should mention 'DANGER', got: {req:?}"
                        );
                    }
                    other => panic!("expected Request error, got: {other:?}"),
                }
                anyhow::Ok(())
            }
            .boxed(),
        )
        .await?;
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn mixed_batch(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let config = SqlGitSourceOfTruthConfigBuilder::with_sqlite_in_memory()?.build();
        config
            .insert_repos(
                &ctx,
                &[
                    (
                        RepositoryId::new(1),
                        RepositoryName("repo/a".to_string()),
                        GitSourceOfTruth::Reserved,
                    ),
                    (
                        RepositoryId::new(2),
                        RepositoryName("repo/b".to_string()),
                        GitSourceOfTruth::Reserved,
                    ),
                ],
            )
            .await?;
        // Stamp the two reserved repos with DIFFERENT mutation ids.
        config
            .update_mutation_id_by_repo_names_for_reserved_repos(
                &ctx,
                &[RepositoryName("repo/a".to_string())],
                100,
            )
            .await?;
        config
            .update_mutation_id_by_repo_names_for_reserved_repos(
                &ctx,
                &[RepositoryName("repo/b".to_string())],
                200,
            )
            .await?;

        let params = params_for(&["repo/a", "repo/b"]);
        with_just_knobs_async(
            JustKnobsInMemory::new(HashMap::from([(
                ATTACH_JK.to_string(),
                KnobVal::Bool(true),
            )])),
            async {
                let err = reserve_repos_ids(ctx.clone(), &config, &params)
                    .await
                    .expect_err("expected an error for a batch spanning multiple mutations");
                match &err {
                    scs_errors::ServiceError::Request(req) => {
                        assert!(
                            format!("{req:?}").contains("multiple in-flight mutations"),
                            "message should mention 'multiple in-flight mutations', got: {req:?}"
                        );
                    }
                    other => panic!("expected Request error, got: {other:?}"),
                }
                anyhow::Ok(())
            }
            .boxed(),
        )
        .await?;
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn jk_off(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let config = SqlGitSourceOfTruthConfigBuilder::with_sqlite_in_memory()?.build();
        config
            .insert_repos(
                &ctx,
                &[(
                    RepositoryId::new(1),
                    RepositoryName("repo/a".to_string()),
                    GitSourceOfTruth::Reserved,
                )],
            )
            .await?;
        config
            .update_mutation_id_by_repo_names_for_reserved_repos(
                &ctx,
                &[RepositoryName("repo/a".to_string())],
                4242,
            )
            .await?;

        let params = params_for(&["repo/a"]);
        with_just_knobs_async(
            JustKnobsInMemory::new(HashMap::from([(
                ATTACH_JK.to_string(),
                KnobVal::Bool(false),
            )])),
            async {
                let err = reserve_repos_ids(ctx.clone(), &config, &params)
                    .await
                    .expect_err("expected today's behavior (error) when the knob is off");
                assert!(
                    matches!(err, scs_errors::ServiceError::Request(_)),
                    "expected Request error, got: {err:?}"
                );
                anyhow::Ok(())
            }
            .boxed(),
        )
        .await?;
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn attach_happy_path_multi_repo(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let config = SqlGitSourceOfTruthConfigBuilder::with_sqlite_in_memory()?.build();
        // Two reserved repos both stamped with the SAME mutation id: a
        // duplicate multi-repo request should dedup to one mutation and attach.
        config
            .insert_repos(
                &ctx,
                &[
                    (
                        RepositoryId::new(1),
                        RepositoryName("repo/a".to_string()),
                        GitSourceOfTruth::Reserved,
                    ),
                    (
                        RepositoryId::new(2),
                        RepositoryName("repo/b".to_string()),
                        GitSourceOfTruth::Reserved,
                    ),
                ],
            )
            .await?;
        config
            .update_mutation_id_by_repo_names_for_reserved_repos(
                &ctx,
                &[
                    RepositoryName("repo/a".to_string()),
                    RepositoryName("repo/b".to_string()),
                ],
                4242,
            )
            .await?;

        let params = params_for(&["repo/a", "repo/b"]);
        with_just_knobs_async(
            JustKnobsInMemory::new(HashMap::from([(
                ATTACH_JK.to_string(),
                KnobVal::Bool(true),
            )])),
            async {
                let outcome = reserve_repos_ids(ctx.clone(), &config, &params)
                    .await
                    .expect("reserve_repos_ids should succeed and attach for multi-repo");
                match outcome {
                    ReserveOutcome::AttachedToInflight { mutation_id } => {
                        assert_eq!(mutation_id, 4242);
                    }
                    ReserveOutcome::Reserved(_) => {
                        panic!("expected AttachedToInflight, got Reserved")
                    }
                }
                anyhow::Ok(())
            }
            .boxed(),
        )
        .await?;
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn attach_lookup_none(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let config = SqlGitSourceOfTruthConfigBuilder::with_sqlite_in_memory()?.build();
        // Only one of the two requested repos has a seeded (reserved+stamped)
        // row; the other has NO row at all. The absent repo makes the batch
        // non-attachable (lookup returns None => `any_non_reserved`), so the
        // whole request must fail closed rather than attaching to the single
        // reserved mutation.
        config
            .insert_repos(
                &ctx,
                &[(
                    RepositoryId::new(1),
                    RepositoryName("repo/a".to_string()),
                    GitSourceOfTruth::Reserved,
                )],
            )
            .await?;
        config
            .update_mutation_id_by_repo_names_for_reserved_repos(
                &ctx,
                &[RepositoryName("repo/a".to_string())],
                4242,
            )
            .await?;

        let params = params_for(&["repo/a", "repo/absent"]);
        with_just_knobs_async(
            JustKnobsInMemory::new(HashMap::from([(
                ATTACH_JK.to_string(),
                KnobVal::Bool(true),
            )])),
            async {
                let err = reserve_repos_ids(ctx.clone(), &config, &params)
                    .await
                    .expect_err("expected an error when a requested repo has no row");
                assert!(
                    matches!(err, scs_errors::ServiceError::Request(_)),
                    "expected Request error, got: {err:?}"
                );
                anyhow::Ok(())
            }
            .boxed(),
        )
        .await?;
        Ok(())
    }

    #[mononoke::test]
    fn batch_at_cap_is_allowed() {
        check_batch_size(2, 2).expect("a batch exactly at the cap must be allowed");
    }

    #[mononoke::test]
    fn batch_over_cap_is_rejected() {
        let err = check_batch_size(3, 2).expect_err("a batch over the cap must be rejected");
        // An Internal error would be retried by `create_repos_in_mononoke`
        // rather than reported to the caller.
        let scs_errors::ServiceError::Request(request_error) = &err else {
            panic!("expected Request error, got: {err:?}");
        };
        let message = format!("{request_error:?}");
        assert!(
            message.contains('3'),
            "message should state the requested count, got: {message}"
        );
        assert!(
            message.contains('2'),
            "message should state the cap, got: {message}"
        );
    }

    #[mononoke::test]
    fn empty_batch_is_rejected() {
        let err = check_batch_size(0, BatchSizeTier::Default.max_batch_size())
            .expect_err("an empty batch must be rejected");
        let scs_errors::ServiceError::Request(request_error) = &err else {
            panic!("expected Request error, got: {err:?}");
        };
        let message = format!("{request_error:?}");
        assert!(
            message.contains("empty"),
            "message should say the batch was empty, got: {message}"
        );
    }

    #[mononoke::test]
    fn tiers_admit_the_batches_they_are_sized_for() {
        let elevated = BatchSizeTier::Elevated.max_batch_size();
        let default = BatchSizeTier::Default.max_batch_size();

        // Real runs through the bulk-import pipeline: ~40, 95, 134.
        for count in [40, 95, 134] {
            check_batch_size(count, elevated).unwrap_or_else(|err| {
                panic!("a pipeline batch of {count} must be allowed, got: {err:?}")
            });
        }
        // p50 of all observed traffic is 1: the common case is one repo.
        check_batch_size(1, default).expect("a single-repo batch must always be allowed");
    }

    #[mononoke::test]
    fn only_source_control_admits_the_largest_batches_on_record() {
        // 588 (par-msl, the all-time high) and the 700+ MTK Wearables say they
        // expect. Source Control has issued batches this size and can still do
        // so; a delegated grant deliberately cannot, so a group member has to
        // split, ask for the cap to be raised, or hand the batch back to Source
        // Control. Sizing the delegated tier around its two largest outliers
        // would leave it bounding nothing. Encoded so the decision is
        // discoverable rather than a surprise at the flip.
        let source_control = BatchSizeTier::SourceControl.max_batch_size();
        let elevated = BatchSizeTier::Elevated.max_batch_size();
        for count in [588, 700] {
            check_batch_size(count, source_control).unwrap_or_else(|err| {
                panic!("Source Control must still be able to issue {count}, got: {err:?}")
            });
            check_batch_size(count, elevated)
                .expect_err(&format!("{count} is expected to exceed the elevated tier"));
        }
    }

    #[mononoke::test]
    fn the_default_tier_does_not_admit_the_observed_p95() {
        // Also deliberate, and the sharper edge of the two. Observed over 32
        // days: p95 66, max 139 -- both from ad-hoc Thrift Fiddle onboarding
        // runs. Under a default of 50 those callers must split their batches,
        // join the group, or be recognised as Source Control, which the
        // identity check should already do for a human on the team. Anyone else
        // doing bulk creation from an unrecognised identity is exactly what the
        // cap is for, so this is the intended bite rather than collateral.
        let default = BatchSizeTier::Default.max_batch_size();
        check_batch_size(66, default).expect_err("the observed p95 is expected to exceed default");
        check_batch_size(139, default).expect_err("the observed max is expected to exceed default");
    }
}

#[cfg(all(fbcode_build, test))]
mod symref_tests {
    use std::collections::HashMap;

    use fbinit::FacebookInit;
    use futures::FutureExt;
    use git_source_of_truth::SqlGitSourceOfTruthConfigBuilder;
    use git_symbolic_refs::RefType;
    use git_symbolic_refs::SqlGitSymbolicRefsBuilder;
    use justknobs::test_helpers::JustKnobsInMemory;
    use justknobs::test_helpers::KnobVal;
    use justknobs::test_helpers::with_just_knobs_async;
    use mononoke_macros::mononoke;
    use sql_construct::SqlConstruct;

    use super::*;

    struct TestSymrefStoreProvider {
        stores: HashMap<RepositoryId, Arc<dyn GitSymbolicRefs>>,
    }

    #[async_trait::async_trait]
    impl SymrefStoreProvider for TestSymrefStoreProvider {
        async fn repo_store(
            &self,
            repo_id: RepositoryId,
        ) -> Result<Arc<dyn GitSymbolicRefs>, scs_errors::ServiceError> {
            self.stores.get(&repo_id).cloned().ok_or_else(|| {
                scs_errors::internal_error(format!("no test symref store for repo {repo_id}"))
                    .into()
            })
        }
    }

    struct FailingSymrefStore {
        repo_id: RepositoryId,
    }

    #[async_trait::async_trait]
    impl GitSymbolicRefs for FailingSymrefStore {
        fn repo_id(&self) -> RepositoryId {
            self.repo_id
        }

        async fn get_ref_by_symref(
            &self,
            _ctx: &CoreContext,
            _symref: String,
        ) -> Result<Option<GitSymbolicRefsEntry>> {
            anyhow::bail!("FailingSymrefStore always fails")
        }

        async fn get_symrefs_by_ref(
            &self,
            _ctx: &CoreContext,
            _ref_name: String,
            _ref_type: RefType,
        ) -> Result<Option<Vec<String>>> {
            anyhow::bail!("FailingSymrefStore always fails")
        }

        async fn add_or_update_entries(
            &self,
            _ctx: &CoreContext,
            _entries: Vec<GitSymbolicRefsEntry>,
        ) -> Result<()> {
            anyhow::bail!("FailingSymrefStore always fails")
        }

        async fn delete_symrefs(&self, _ctx: &CoreContext, _symrefs: Vec<String>) -> Result<()> {
            anyhow::bail!("FailingSymrefStore always fails")
        }

        async fn list_all_symrefs(&self, _ctx: &CoreContext) -> Result<Vec<GitSymbolicRefsEntry>> {
            anyhow::bail!("FailingSymrefStore always fails")
        }
    }

    fn sqlite_store(repo_id: RepositoryId) -> Result<Arc<dyn GitSymbolicRefs>> {
        Ok(Arc::new(
            SqlGitSymbolicRefsBuilder::with_sqlite_in_memory()?.build(repo_id),
        ))
    }

    fn request_with_branch(name: &str, branch: Option<&str>) -> thrift::RepoCreationRequest {
        thrift::RepoCreationRequest {
            repo_name: name.to_string(),
            scm_type: thrift::RepoScmType::GIT,
            size_bucket: thrift::RepoSizeBucket::SMALL,
            default_branch: branch.map(str::to_string),
            ..Default::default()
        }
    }

    fn requests_of(
        repo_ids_and_requests: &[(RepositoryId, thrift::RepoCreationRequest)],
    ) -> Vec<thrift::RepoCreationRequest> {
        repo_ids_and_requests
            .iter()
            .map(|(_id, request)| request.clone())
            .collect()
    }

    async fn head_branch(
        ctx: &CoreContext,
        store: &Arc<dyn GitSymbolicRefs>,
    ) -> Result<Option<String>> {
        Ok(store
            .get_ref_by_symref(ctx, HEAD_SYMREF.to_string())
            .await?
            .map(|entry| entry.ref_name))
    }

    fn symref_jk(enabled: bool) -> JustKnobsInMemory {
        JustKnobsInMemory::new(HashMap::from([(
            WRITE_DEFAULT_BRANCH_SYMREF_JK.to_string(),
            KnobVal::Bool(enabled),
        )]))
    }

    #[mononoke::fbinit_test]
    async fn writes_head_only_for_requests_with_default_branch(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let store_with = sqlite_store(RepositoryId::new(1))?;
        let store_without = sqlite_store(RepositoryId::new(2))?;
        let stores = TestSymrefStoreProvider {
            stores: HashMap::from([
                (RepositoryId::new(1), store_with.clone()),
                (RepositoryId::new(2), store_without.clone()),
            ]),
        };
        let repo_ids_and_requests = vec![
            (
                RepositoryId::new(1),
                request_with_branch("repo/with", Some("main")),
            ),
            (
                RepositoryId::new(2),
                request_with_branch("repo/without", None),
            ),
        ];

        with_just_knobs_async(
            symref_jk(true),
            async {
                let enabled = validate_default_branches(&requests_of(&repo_ids_and_requests))
                    .expect("valid default branches must pass validation");
                assert!(
                    enabled,
                    "JK on + a set default_branch must enable symref writes",
                );
                let written =
                    write_default_branch_symrefs(&ctx, &stores, &repo_ids_and_requests, enabled)
                        .await
                        .expect("writing symrefs for a mixed batch should succeed");
                assert_eq!(
                    written,
                    vec![RepositoryId::new(1)],
                    "only the repo with default_branch should be reported as written",
                );
                anyhow::Ok(())
            }
            .boxed(),
        )
        .await?;

        assert_eq!(
            head_branch(&ctx, &store_with).await?,
            Some("main".to_string()),
            "the repo with default_branch must get a HEAD row",
        );
        assert_eq!(
            head_branch(&ctx, &store_without).await?,
            None,
            "the repo without default_branch must NOT get a HEAD row",
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn jk_off_writes_nothing(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let store = sqlite_store(RepositoryId::new(1))?;
        let stores = TestSymrefStoreProvider {
            stores: HashMap::from([(RepositoryId::new(1), store.clone())]),
        };
        let repo_ids_and_requests = vec![(
            RepositoryId::new(1),
            request_with_branch("repo/with", Some("main")),
        )];

        with_just_knobs_async(
            symref_jk(false),
            async {
                let enabled = validate_default_branches(&requests_of(&repo_ids_and_requests))
                    .expect("the gated-off path must not reject the request");
                assert!(!enabled, "writes must be disabled with the kill switch off");
                let written =
                    write_default_branch_symrefs(&ctx, &stores, &repo_ids_and_requests, enabled)
                        .await
                        .expect("the gated-off path should succeed as a no-op");
                assert!(
                    written.is_empty(),
                    "nothing should be reported as written with the kill switch off",
                );
                anyhow::Ok(())
            }
            .boxed(),
        )
        .await?;

        assert_eq!(
            head_branch(&ctx, &store).await?,
            None,
            "no HEAD row should be written with the kill switch off",
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn jk_off_invalid_branch_is_fully_inert(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let store = sqlite_store(RepositoryId::new(1))?;
        let stores = TestSymrefStoreProvider {
            stores: HashMap::from([(RepositoryId::new(1), store.clone())]),
        };
        let repo_ids_and_requests = vec![(
            RepositoryId::new(1),
            request_with_branch("repo/with", Some("refs/heads/x")),
        )];

        with_just_knobs_async(
            symref_jk(false),
            async {
                let enabled = validate_default_branches(&requests_of(&repo_ids_and_requests))
                    .expect(
                        "an invalid default_branch must NOT be rejected with the kill switch off",
                    );
                assert!(!enabled, "writes must be disabled with the kill switch off");
                let written =
                    write_default_branch_symrefs(&ctx, &stores, &repo_ids_and_requests, enabled)
                        .await
                        .expect("the gated-off path should succeed as a no-op");
                assert!(
                    written.is_empty(),
                    "nothing should be reported as written with the kill switch off",
                );
                anyhow::Ok(())
            }
            .boxed(),
        )
        .await?;

        assert_eq!(
            head_branch(&ctx, &store).await?,
            None,
            "no HEAD row should be written with the kill switch off",
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn jk_on_invalid_branch_is_rejected(_fb: FacebookInit) -> Result<()> {
        with_just_knobs_async(
            symref_jk(true),
            async {
                let err = validate_default_branches(&[request_with_branch(
                    "repo/with",
                    Some("refs/heads/x"),
                )])
                .expect_err("an invalid default_branch must be rejected with the kill switch on");
                assert!(
                    matches!(err, scs_errors::ServiceError::Request(_)),
                    "expected Request error, got: {err:?}"
                );
                anyhow::Ok(())
            }
            .boxed(),
        )
        .await?;
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn no_default_branch_targets_short_circuit_before_jk(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let store = sqlite_store(RepositoryId::new(1))?;
        let stores = TestSymrefStoreProvider {
            stores: HashMap::from([(RepositoryId::new(1), store.clone())]),
        };
        let repo_ids_and_requests = vec![(
            RepositoryId::new(1),
            request_with_branch("repo/without", None),
        )];

        // Deliberately no `with_just_knobs_async`: a JK eval against the unset test store panics.
        let enabled = validate_default_branches(&requests_of(&repo_ids_and_requests))
            .expect("a batch with no default_branch must succeed without a JK eval");
        assert!(!enabled, "a batch with no default_branch enables nothing");
        let written = write_default_branch_symrefs(&ctx, &stores, &repo_ids_and_requests, enabled)
            .await
            .expect("a batch with no default_branch must succeed without a JK eval");
        assert!(
            written.is_empty(),
            "nothing should be reported as written for a batch with no default_branch",
        );
        assert_eq!(
            head_branch(&ctx, &store).await?,
            None,
            "no HEAD row should be written for a batch with no default_branch",
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn poll_cleanup_deletes_symrefs_and_sot_rows(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let sot = SqlGitSourceOfTruthConfigBuilder::with_sqlite_in_memory()?.build();
        sot.insert_repos(
            &ctx,
            &[
                (
                    RepositoryId::new(1),
                    RepositoryName("repo/a".to_string()),
                    GitSourceOfTruth::Reserved,
                ),
                (
                    RepositoryId::new(2),
                    RepositoryName("repo/b".to_string()),
                    GitSourceOfTruth::Reserved,
                ),
            ],
        )
        .await?;
        sot.update_mutation_id_by_repo_names_for_reserved_repos(
            &ctx,
            &[
                RepositoryName("repo/a".to_string()),
                RepositoryName("repo/b".to_string()),
            ],
            4242,
        )
        .await?;

        let store_a = sqlite_store(RepositoryId::new(1))?;
        let store_b = sqlite_store(RepositoryId::new(2))?;
        write_default_branch_symref(&ctx, store_a.as_ref(), "main")
            .await
            .expect("seeding repo/a's HEAD row should succeed");
        write_default_branch_symref(&ctx, store_b.as_ref(), "main")
            .await
            .expect("seeding repo/b's HEAD row should succeed");
        let stores = TestSymrefStoreProvider {
            stores: HashMap::from([
                (RepositoryId::new(1), store_a.clone()),
                (RepositoryId::new(2), store_b.clone()),
            ]),
        };

        cleanup_repos(ctx.clone(), &sot, &stores, 4242)
            .await
            .expect("poll-path cleanup should succeed");

        assert_eq!(
            head_branch(&ctx, &store_a).await?,
            None,
            "cleanup must delete repo/a's HEAD row",
        );
        assert_eq!(
            head_branch(&ctx, &store_b).await?,
            None,
            "cleanup must delete repo/b's HEAD row",
        );
        for name in ["repo/a", "repo/b"] {
            assert!(
                sot.get_by_repo_name(
                    &ctx,
                    &RepositoryName(name.to_string()),
                    Staleness::MostRecent,
                )
                .await?
                .is_none(),
                "cleanup must delete the reserved SoT row for {name}",
            );
        }
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn poll_cleanup_with_unseeded_symref_store_succeeds(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let sot = SqlGitSourceOfTruthConfigBuilder::with_sqlite_in_memory()?.build();
        sot.insert_repos(
            &ctx,
            &[(
                RepositoryId::new(1),
                RepositoryName("repo/a".to_string()),
                GitSourceOfTruth::Reserved,
            )],
        )
        .await?;
        sot.update_mutation_id_by_repo_names_for_reserved_repos(
            &ctx,
            &[RepositoryName("repo/a".to_string())],
            4242,
        )
        .await?;

        let store = sqlite_store(RepositoryId::new(1))?;
        let stores = TestSymrefStoreProvider {
            stores: HashMap::from([(RepositoryId::new(1), store.clone())]),
        };

        cleanup_repos(ctx.clone(), &sot, &stores, 4242)
            .await
            .expect("cleanup against a store with no HEAD row must succeed");

        assert_eq!(
            head_branch(&ctx, &store).await?,
            None,
            "there is still no HEAD row after cleanup",
        );
        assert!(
            sot.get_by_repo_name(
                &ctx,
                &RepositoryName("repo/a".to_string()),
                Staleness::MostRecent,
            )
            .await?
            .is_none(),
            "cleanup must still delete the reserved SoT row",
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn prepare_failure_cleanup_deletes_symrefs_and_sot_rows(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let sot = SqlGitSourceOfTruthConfigBuilder::with_sqlite_in_memory()?.build();
        sot.insert_repos(
            &ctx,
            &[(
                RepositoryId::new(1),
                RepositoryName("repo/a".to_string()),
                GitSourceOfTruth::Reserved,
            )],
        )
        .await?;

        let store = sqlite_store(RepositoryId::new(1))?;
        write_default_branch_symref(&ctx, store.as_ref(), "main")
            .await
            .expect("seeding the HEAD row should succeed");
        let stores = TestSymrefStoreProvider {
            stores: HashMap::from([(RepositoryId::new(1), store.clone())]),
        };
        let params = thrift::CreateReposParams {
            repos: vec![request_with_branch("repo/a", Some("main"))],
            ..Default::default()
        };

        cleanup_reserved_repos_after_failure(&ctx, &sot, &stores, &[RepositoryId::new(1)], &params)
            .await
            .expect("prepare-failure cleanup should succeed");

        assert_eq!(
            head_branch(&ctx, &store).await?,
            None,
            "prepare-failure cleanup must delete the HEAD row",
        );
        assert!(
            sot.get_by_repo_name(
                &ctx,
                &RepositoryName("repo/a".to_string()),
                Staleness::MostRecent,
            )
            .await?
            .is_none(),
            "prepare-failure cleanup must delete the reserved SoT row",
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn poll_cleanup_failed_symref_delete_keeps_sot_rows(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let sot = SqlGitSourceOfTruthConfigBuilder::with_sqlite_in_memory()?.build();
        sot.insert_repos(
            &ctx,
            &[(
                RepositoryId::new(1),
                RepositoryName("repo/a".to_string()),
                GitSourceOfTruth::Reserved,
            )],
        )
        .await?;
        sot.update_mutation_id_by_repo_names_for_reserved_repos(
            &ctx,
            &[RepositoryName("repo/a".to_string())],
            7,
        )
        .await?;

        let stores = TestSymrefStoreProvider {
            stores: HashMap::from([(
                RepositoryId::new(1),
                Arc::new(FailingSymrefStore {
                    repo_id: RepositoryId::new(1),
                }) as Arc<dyn GitSymbolicRefs>,
            )]),
        };

        cleanup_repos(ctx.clone(), &sot, &stores, 7)
            .await
            .expect_err("cleanup must fail when the symref delete fails");

        let entry = sot
            .get_by_repo_name(
                &ctx,
                &RepositoryName("repo/a".to_string()),
                Staleness::MostRecent,
            )
            .await?
            .expect("the SoT row must survive a failed symref delete");
        assert_eq!(
            entry.source_of_truth,
            GitSourceOfTruth::Reserved,
            "the surviving row must still be Reserved (retryable)",
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn prepare_failure_cleanup_failed_symref_delete_keeps_sot_rows(
        fb: FacebookInit,
    ) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let sot = SqlGitSourceOfTruthConfigBuilder::with_sqlite_in_memory()?.build();
        sot.insert_repos(
            &ctx,
            &[(
                RepositoryId::new(1),
                RepositoryName("repo/a".to_string()),
                GitSourceOfTruth::Reserved,
            )],
        )
        .await?;

        let stores = TestSymrefStoreProvider {
            stores: HashMap::from([(
                RepositoryId::new(1),
                Arc::new(FailingSymrefStore {
                    repo_id: RepositoryId::new(1),
                }) as Arc<dyn GitSymbolicRefs>,
            )]),
        };
        let params = thrift::CreateReposParams {
            repos: vec![request_with_branch("repo/a", Some("main"))],
            ..Default::default()
        };

        cleanup_reserved_repos_after_failure(&ctx, &sot, &stores, &[RepositoryId::new(1)], &params)
            .await
            .expect_err("prepare-failure cleanup must fail when the symref delete fails");

        let entry = sot
            .get_by_repo_name(
                &ctx,
                &RepositoryName("repo/a".to_string()),
                Staleness::MostRecent,
            )
            .await?
            .expect("the SoT row must survive a failed symref delete");
        assert_eq!(
            entry.source_of_truth,
            GitSourceOfTruth::Reserved,
            "the surviving row must still be Reserved (retryable)",
        );
        Ok(())
    }
}
