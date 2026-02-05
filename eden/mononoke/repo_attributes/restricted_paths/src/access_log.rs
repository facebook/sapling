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

/// Check if the caller has access to paths protected by the given ACLs.
pub async fn has_access_to_acl(
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

// ============================================================================
// Schematized logger implementation (fbcode_build only)
// ============================================================================

#[cfg(fbcode_build)]
mod schematized_logger {
    use std::env::var;

    use anyhow::Result;
    use context::CoreContext;
    use fbwhoami::FbWhoAmI;
    use hostname::get_hostname;
    use mononoke_restricted_paths_access_rust_logger::MononokeRestrictedPathsAccessLogger;
    use mononoke_types::NonRootMPath;
    use mononoke_types::RepositoryId;
    use permission_checker::MononokeIdentity;

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
        acls: &[&MononokeIdentity],
    ) -> Result<()> {
        let mut logger = MononokeRestrictedPathsAccessLogger::new(ctx.fb);

        // Add common server data (equivalent to scuba.add_common_server_data())
        add_common_server_data(&mut logger);

        // Add metadata (equivalent to scuba.add_metadata())
        add_metadata(&mut logger, ctx);

        // Set core access fields
        logger.set_repo_id(repo_id.id() as i64);
        logger.set_restricted_paths(
            restricted_paths
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>(),
        );
        logger.set_has_authorization(has_authorization.to_string());
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

    /// Add common server data to the schematized logger.
    /// Mirrors the behavior of ScubaSampleBuilder::add_common_server_data().
    fn add_common_server_data(logger: &mut MononokeRestrictedPathsAccessLogger) {
        if let Ok(hostname) = get_hostname() {
            logger.set_server_hostname(hostname);
        }

        if let Ok(who) = FbWhoAmI::get() {
            if let Some(region) = who.region.as_deref() {
                logger.set_region(region.to_owned());
            }
            if let Some(dc) = who.datacenter.as_deref() {
                logger.set_datacenter(dc.to_owned());
            }
            if let Some(dc_prefix) = who.region_datacenter_prefix.as_deref() {
                logger.set_region_datacenter_prefix(dc_prefix.to_owned());
            }
        }

        if let Ok(smc_tier) = var("SMC_TIERS") {
            logger.set_server_tier(smc_tier);
        }

        if let Ok(tw_task_id) = var("TW_TASK_ID") {
            logger.set_tw_task_id(tw_task_id);
        }

        if let Ok(tw_canary_id) = var("TW_CANARY_ID") {
            logger.set_tw_canary_id(tw_canary_id);
        }

        if let (Ok(tw_cluster), Ok(tw_user), Ok(tw_name)) = (
            var("TW_JOB_CLUSTER"),
            var("TW_JOB_USER"),
            var("TW_JOB_NAME"),
        ) {
            logger.set_tw_handle(format!("{}/{}/{}", tw_cluster, tw_user, tw_name));

            if let Ok(tw_task_id) = var("TW_TASK_ID") {
                logger.set_tw_task_handle(format!(
                    "{}/{}/{}/{}",
                    tw_cluster, tw_user, tw_name, tw_task_id
                ));
            }
        }

        if let Ok(cluster) = var("CHRONOS_CLUSTER") {
            logger.set_chronos_cluster(cluster);
        }

        if let Ok(id) = var("CHRONOS_JOB_INSTANCE_ID") {
            logger.set_chronos_job_instance_id(id);
        }

        if let Ok(job_name) = var("CHRONOS_JOB_NAME") {
            logger.set_chronos_job_name(job_name);
        }

        logger.set_build_revision(build_info::BuildInfo::get_revision().to_string());
        logger.set_build_rule(build_info::BuildInfo::get_rule().to_string());
    }

    /// Add metadata fields to the schematized logger.
    /// Mirrors the behavior of MononokeScubaSampleBuilder::add_metadata().
    fn add_metadata(logger: &mut MononokeRestrictedPathsAccessLogger, ctx: &CoreContext) {
        let metadata = ctx.metadata();

        logger.set_session_uuid(metadata.session_id().to_string());

        logger.set_client_identities(
            metadata
                .identities()
                .iter()
                .map(|i| i.to_string())
                .collect(),
        );

        if let Some(first_identity) = metadata.identities().first() {
            logger.set_client_identity_variant(first_identity.variant().to_string());
        }

        if let Some(client_hostname) = metadata.client_hostname() {
            logger.set_source_hostname(client_hostname.to_owned());
        } else if let Some(client_ip) = metadata.client_ip() {
            logger.set_client_ip(client_ip.to_string());
        }

        if let Some(unix_name) = metadata.unix_name() {
            logger.set_unix_username(unix_name.to_string());
        }

        // Add client request info if available
        if let Some(cri) = ctx.client_request_info() {
            if let Some(main_id) = &cri.main_id {
                logger.set_client_main_id(main_id.clone());
            }
            logger.set_client_entry_point(cri.entry_point.to_string());
            logger.set_client_correlator(cri.correlator.clone());

            // Add enabled experiments JKs
            let enabled_experiments_jk =
                scuba_ext::MononokeScubaSampleBuilder::get_enabled_experiments_jk(cri);
            logger.set_enabled_experiments_jk(enabled_experiments_jk);
        }

        if let Some(sandcastle_alias) = metadata.sandcastle_alias() {
            logger.set_sandcastle_alias(sandcastle_alias.to_string());
        }
        if let Some(sandcastle_vcs) = metadata.sandcastle_vcs() {
            logger.set_sandcastle_vcs(sandcastle_vcs.to_string());
        }
        if let Some(revproxy_region) = metadata.revproxy_region() {
            logger.set_revproxy_region(revproxy_region.to_string());
        }
        if let Some(sandcastle_nonce) = metadata.sandcastle_nonce() {
            logger.set_sandcastle_nonce(sandcastle_nonce.to_string());
        }
        if let Some(tw_job) = metadata.clientinfo_tw_job() {
            logger.set_client_tw_job(tw_job.to_string());
        }
        if let Some(tw_task) = metadata.clientinfo_tw_task() {
            logger.set_client_tw_task(tw_task.to_string());
        }
        if let Some(atlas) = metadata.clientinfo_atlas() {
            logger.set_client_atlas(atlas.to_string());
        }
        if let Some(atlas_env_id) = metadata.clientinfo_atlas_env_id() {
            logger.set_client_atlas_env_id(atlas_env_id.to_string());
        }
        if let Some(fetch_cause) = metadata.fetch_cause() {
            logger.set_fetch_cause(fetch_cause.to_string());
        }
        logger.set_fetch_from_cas_attempted(metadata.fetch_from_cas_attempted());
    }
}

pub(crate) async fn log_access_to_restricted_path(
    ctx: &CoreContext,
    repo_id: RepositoryId,
    restricted_paths: Vec<NonRootMPath>,
    acls: Vec<&MononokeIdentity>,
    access_data: RestrictedPathAccessData,
    acl_provider: Arc<dyn AclProvider>,
    scuba: MononokeScubaSampleBuilder,
) -> Result<bool> {
    // TODO(T239041722): store permission checkers in RestrictedPaths to improve
    // performance if needed.
    let has_authorization = has_access_to_acl(ctx, &acl_provider, &acls).await?;

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
                &acls,
            ) {
                tracing::error!("Failed to log to schematized logger: {:?}", e);
            }
        }
    }

    // Keep existing Scuba logging during migration for safety.
    // This can be removed once the schematized logger is verified in production.
    log_access_to_scuba(
        ctx,
        repo_id,
        restricted_paths,
        access_data,
        has_authorization,
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
