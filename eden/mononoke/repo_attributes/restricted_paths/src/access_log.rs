/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// Log access to restricted paths

use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use context::CoreContext;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use mononoke_types::NonRootMPath;
use mononoke_types::RepositoryId;
use permission_checker::AclProvider;
use permission_checker::MononokeIdentity;
use permission_checker::PermissionCheckerBuilder;
use scuba_ext::MononokeScubaSampleBuilder;

use crate::manifest_id_store::ManifestId;
use crate::manifest_id_store::ManifestType;

pub const ACCESS_LOG_SCUBA_TABLE: &str = "mononoke_restricted_paths_access_test";

pub(crate) enum RestrictedPathAccessData {
    /// When the tree is accessed by manifest id
    Manifest(ManifestId, ManifestType),
    /// When the tree is accessed by path
    FullPath { full_path: NonRootMPath },
}

/// Check if the caller has read access to paths protected by the given repo region ACLs.
/// This uses PermissionChecker to verify the caller has "read" permission.
pub async fn has_read_access_to_repo_region_acls(
    ctx: &CoreContext,
    acl_provider: &Arc<dyn AclProvider>,
    acls: &[&MononokeIdentity],
) -> Result<bool> {
    if acls.is_empty() {
        return Ok(true);
    }

    let permission_checker = stream::iter(acls.iter().cloned())
        .map(anyhow::Ok)
        .try_fold(PermissionCheckerBuilder::new(), async |builder, acl| {
            Ok(builder.allow(
                acl_provider
                    .repo_region_acl(acl.id_data())
                    .await
                    .with_context(|| {
                        format!("Failed to create PermissionChecker for {}", acl.id_data())
                    })?,
            ))
        })
        .await
        .context("creating PermissionCheckerBuilder")?
        .build();

    Ok(permission_checker
        .check_set(ctx.metadata().identities(), &["read"])
        .await)
}

/// Check if the caller is a member of any of the given groups.
/// Returns true if the caller is a member of at least one group (OR logic).
/// If the groups list is empty, returns true (no restriction).
pub async fn is_member_of_groups(
    ctx: &CoreContext,
    acl_provider: &Arc<dyn AclProvider>,
    groups: &[&MononokeIdentity],
) -> Result<bool> {
    if groups.is_empty() {
        return Ok(true);
    }

    // Run all group membership checks concurrently
    let membership_results: Vec<bool> = stream::iter(groups.iter().cloned())
        .map(|group| async move {
            let membership_checker =
                acl_provider.group(group.id_data()).await.with_context(|| {
                    format!("Failed to create MembershipChecker for {}", group.id_data())
                })?;
            anyhow::Ok(
                membership_checker
                    .is_member(ctx.metadata().identities())
                    .await,
            )
        })
        .boxed()
        .buffer_unordered(groups.len())
        .try_collect()
        .await?;

    Ok(membership_results.into_iter().any(|m| m))
}

/// Check if the caller is a member of the given group.
pub async fn is_part_of_group(
    ctx: &CoreContext,
    acl_provider: &Arc<dyn AclProvider>,
    group_name: &str,
) -> Result<bool> {
    let membership_checker = acl_provider
        .group(group_name)
        .await
        .with_context(|| format!("Failed to get membership checker for group {}", group_name))?;

    Ok(membership_checker
        .is_member(ctx.metadata().identities())
        .await)
}

// ============================================================================
// Schematized logger implementation (fbcode_build only)
// ============================================================================

#[cfg(fbcode_build)]
mod schematized_logger {
    use anyhow::Result;
    use context::CoreContext;
    use mononoke_restricted_paths_access_rust_logger::MononokeRestrictedPathsAccessLogger;
    use mononoke_types::NonRootMPath;
    use mononoke_types::RepositoryId;
    use permission_checker::MononokeIdentity;
    use scuba_ext::CommonMetadata;
    use scuba_ext::CommonServerData;

    use super::RestrictedPathAccessData;

    /// Log access to the schematized logger for restricted paths.
    ///
    /// This logs to both Scuba and Hive via the MononokeRestrictedPathsAccessLogger.
    pub fn log_access_to_schematized_logger(
        ctx: &CoreContext,
        repo_id: RepositoryId,
        restricted_paths: &[NonRootMPath],
        access_data: &RestrictedPathAccessData,
        has_authorization: bool,
        is_allowlisted_tooling: bool,
        acls: &[&MononokeIdentity],
    ) -> Result<()> {
        let mut logger = MononokeRestrictedPathsAccessLogger::new(ctx.fb);

        // Add common server data using shared struct
        let server_data = CommonServerData::collect();
        apply_server_data(&mut logger, &server_data);

        // Add metadata using shared struct
        let metadata = CommonMetadata::from_metadata(ctx.metadata());
        apply_metadata(&mut logger, &metadata);

        // Set core access fields
        logger.set_repo_id(repo_id.id() as i64);
        logger.set_restricted_paths(
            restricted_paths
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>(),
        );
        logger.set_has_authorization(has_authorization.to_string());
        logger.set_is_allowlisted_tooling(is_allowlisted_tooling.to_string());
        logger.set_acls(acls.iter().map(|acl| acl.to_string()).collect::<Vec<_>>());

        // Set access data variant fields
        match access_data {
            RestrictedPathAccessData::Manifest(manifest_id, manifest_type) => {
                logger.set_manifest_id(manifest_id.to_string());
                logger.set_manifest_type(manifest_type.to_string());
            }
            RestrictedPathAccessData::FullPath { full_path } => {
                logger.set_full_path(full_path.to_string());
            }
        }

        logger.log_async()?;
        Ok(())
    }

    /// Apply CommonServerData fields to the schematized logger.
    fn apply_server_data(
        logger: &mut MononokeRestrictedPathsAccessLogger,
        data: &CommonServerData,
    ) {
        if let Some(ref hostname) = data.server_hostname {
            logger.set_server_hostname(hostname.clone());
        }
        if let Some(ref region) = data.region {
            logger.set_region(region.clone());
        }
        if let Some(ref dc) = data.datacenter {
            logger.set_datacenter(dc.clone());
        }
        if let Some(ref dc_prefix) = data.region_datacenter_prefix {
            logger.set_region_datacenter_prefix(dc_prefix.clone());
        }
        if let Some(ref tier) = data.server_tier {
            logger.set_server_tier(tier.clone());
        }
        if let Some(ref tw_task_id) = data.tw_task_id {
            logger.set_tw_task_id(tw_task_id.clone());
        }
        if let Some(ref tw_canary_id) = data.tw_canary_id {
            logger.set_tw_canary_id(tw_canary_id.clone());
        }
        if let Some(ref tw_handle) = data.tw_handle {
            logger.set_tw_handle(tw_handle.clone());
        }
        if let Some(ref tw_task_handle) = data.tw_task_handle {
            logger.set_tw_task_handle(tw_task_handle.clone());
        }
        if let Some(ref cluster) = data.chronos_cluster {
            logger.set_chronos_cluster(cluster.clone());
        }
        if let Some(ref id) = data.chronos_job_instance_id {
            logger.set_chronos_job_instance_id(id.clone());
        }
        if let Some(ref name) = data.chronos_job_name {
            logger.set_chronos_job_name(name.clone());
        }
        if let Some(ref rev) = data.build_revision {
            logger.set_build_revision(rev.clone());
        }
        if let Some(ref rule) = data.build_rule {
            logger.set_build_rule(rule.clone());
        }
    }

    /// Apply CommonMetadata fields to the schematized logger.
    fn apply_metadata(logger: &mut MononokeRestrictedPathsAccessLogger, data: &CommonMetadata) {
        logger.set_session_uuid(data.session_uuid.clone());
        logger.set_client_identities(data.client_identities.clone());

        if let Some(ref variant) = data.client_identity_variant {
            logger.set_client_identity_variant(variant.clone());
        }
        if let Some(ref hostname) = data.source_hostname {
            logger.set_source_hostname(hostname.clone());
        }
        if let Some(ref ip) = data.client_ip {
            logger.set_client_ip(ip.clone());
        }
        if let Some(ref unix_name) = data.unix_username {
            logger.set_unix_username(unix_name.clone());
        }
        if let Some(ref main_id) = data.client_main_id {
            logger.set_client_main_id(main_id.clone());
        }
        if let Some(ref entry_point) = data.client_entry_point {
            logger.set_client_entry_point(entry_point.clone());
        }
        if let Some(ref correlator) = data.client_correlator {
            logger.set_client_correlator(correlator.clone());
        }
        if !data.enabled_experiments_jk.is_empty() {
            logger.set_enabled_experiments_jk(data.enabled_experiments_jk.clone());
        }
        if let Some(ref alias) = data.sandcastle_alias {
            logger.set_sandcastle_alias(alias.clone());
        }
        if let Some(ref vcs) = data.sandcastle_vcs {
            logger.set_sandcastle_vcs(vcs.clone());
        }
        if let Some(ref region) = data.revproxy_region {
            logger.set_revproxy_region(region.clone());
        }
        if let Some(ref nonce) = data.sandcastle_nonce {
            logger.set_sandcastle_nonce(nonce.clone());
        }
        if let Some(ref tw_job) = data.client_tw_job {
            logger.set_client_tw_job(tw_job.clone());
        }
        if let Some(ref tw_task) = data.client_tw_task {
            logger.set_client_tw_task(tw_task.clone());
        }
        if let Some(ref atlas) = data.client_atlas {
            logger.set_client_atlas(atlas.clone());
        }
        if let Some(ref env_id) = data.client_atlas_env_id {
            logger.set_client_atlas_env_id(env_id.clone());
        }
        if let Some(ref cause) = data.fetch_cause {
            logger.set_fetch_cause(cause.clone());
        }
        logger.set_fetch_from_cas_attempted(data.fetch_from_cas_attempted);
    }
}

pub(crate) async fn log_access_to_restricted_path(
    ctx: &CoreContext,
    repo_id: RepositoryId,
    restricted_paths: Vec<NonRootMPath>,
    acls: Vec<&MononokeIdentity>,
    access_data: RestrictedPathAccessData,
    acl_provider: Arc<dyn AclProvider>,
    tooling_allowlist_group: Option<&str>,
    scuba: MononokeScubaSampleBuilder,
) -> Result<bool> {
    // TODO(T239041722): store permission checkers in RestrictedPaths to improve
    // performance if needed.
    let has_path_acl_access =
        has_read_access_to_repo_region_acls(ctx, &acl_provider, &acls).await?;

    // Check if caller is in the tooling allowlist group
    let is_allowlisted_tooling = if let Some(group_name) = tooling_allowlist_group {
        is_part_of_group(ctx, &acl_provider, group_name).await?
    } else {
        false
    };

    // Caller has authorization if they have access via path ACLs OR via tooling allowlist
    let has_authorization = has_path_acl_access || is_allowlisted_tooling;

    // Log to schematized logger (logs to both Scuba and Hive) if enabled via JK
    // Only available in fbcode builds
    #[cfg(fbcode_build)]
    {
        let use_schematized_logger = justknobs::eval(
            "scm/mononoke:restricted_paths_use_schematized_logger",
            None,
            None,
        )?;

        if use_schematized_logger {
            if let Err(e) = schematized_logger::log_access_to_schematized_logger(
                ctx,
                repo_id,
                &restricted_paths,
                &access_data,
                has_authorization,
                is_allowlisted_tooling,
                &acls,
            ) {
                tracing::error!("Failed to log to schematized logger: {:?}", e);
            }
        }
    }

    log_access_to_scuba(
        ctx,
        repo_id,
        restricted_paths,
        access_data,
        has_authorization,
        is_allowlisted_tooling,
        acls,
        scuba,
    )?;

    Ok(has_authorization)
}

fn log_access_to_scuba(
    ctx: &CoreContext,
    repo_id: RepositoryId,
    restricted_paths: Vec<NonRootMPath>,
    access_data: RestrictedPathAccessData,
    has_authorization: bool,
    is_allowlisted_tooling: bool,
    acls: Vec<&MononokeIdentity>,
    mut scuba: MononokeScubaSampleBuilder,
) -> Result<()> {
    scuba.add_metadata(ctx.metadata());

    scuba.add_common_server_data();

    // We want to log all samples
    scuba.unsampled();

    scuba.add("repo_id", repo_id.id());
    scuba.add(
        "restricted_paths",
        restricted_paths
            .into_iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>(),
    );

    scuba.add("has_authorization", has_authorization);
    scuba.add("is_allowlisted_tooling", is_allowlisted_tooling);
    scuba.add(
        "acls",
        acls.into_iter()
            .map(|acl| acl.to_string())
            .collect::<Vec<_>>(),
    );

    // Log access data based on the type
    match access_data {
        RestrictedPathAccessData::Manifest(manifest_id, manifest_type) => {
            scuba.add("manifest_id", manifest_id.to_string());
            scuba.add("manifest_type", manifest_type.to_string());
        }
        RestrictedPathAccessData::FullPath { full_path, .. } => {
            scuba.add("full_path", full_path.to_string());
        }
    }

    scuba.log();

    Ok(())
}
