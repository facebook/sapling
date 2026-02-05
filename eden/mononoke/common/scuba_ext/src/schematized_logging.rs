/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Common data structures for schematized loggers.
//!
//! This module provides reusable structs that collect common server and metadata
//! fields for use with schematized loggers (e.g., MononokeXdbTelemetryLogger,
//! MononokeRestrictedPathsAccessLogger). Since these loggers are generated code
//! with specific setter methods, we provide data structs that callers can use
//! to collect the data once, then apply to their specific logger type.

use std::env::var;

use metadata::Metadata;

use crate::MononokeScubaSampleBuilder;

/// Common server environment data.
///
/// This struct collects server-related fields that are typically set via
/// `add_common_server_data()` on Scuba builders. Use `collect()` to gather
/// all available data from the current environment.
#[derive(Debug, Clone, Default)]
pub struct CommonServerData {
    pub server_hostname: Option<String>,
    pub region: Option<String>,
    pub datacenter: Option<String>,
    pub region_datacenter_prefix: Option<String>,
    pub server_tier: Option<String>,
    pub tw_task_id: Option<String>,
    pub tw_canary_id: Option<String>,
    pub tw_handle: Option<String>,
    pub tw_task_handle: Option<String>,
    pub chronos_cluster: Option<String>,
    pub chronos_job_instance_id: Option<String>,
    pub chronos_job_name: Option<String>,
    pub build_revision: Option<String>,
    pub build_rule: Option<String>,
}

impl CommonServerData {
    /// Collect common server data from the current environment.
    ///
    /// This gathers data from:
    /// - hostname
    /// - fbwhoami (region, datacenter, etc.)
    /// - Environment variables (SMC_TIERS, TW_*, CHRONOS_*)
    /// - Build info
    pub fn collect() -> Self {
        let mut data = Self::default();

        // Server hostname
        if let Ok(hostname) = hostname::get_hostname() {
            data.server_hostname = Some(hostname);
        }

        // Region, datacenter, and region_datacenter_prefix from fbwhoami
        if let Ok(who) = fbwhoami::FbWhoAmI::get() {
            data.region = who.region.clone();
            data.datacenter = who.datacenter.clone();
            data.region_datacenter_prefix = who.region_datacenter_prefix.clone();
        }

        // Server tier from SMC_TIERS environment variable
        if let Ok(smc_tier) = var("SMC_TIERS") {
            data.server_tier = Some(smc_tier);
        }

        // Tupperware task ID
        if let Ok(tw_task_id) = var("TW_TASK_ID") {
            data.tw_task_id = Some(tw_task_id);
        }

        // Tupperware canary ID
        if let Ok(tw_canary_id) = var("TW_CANARY_ID") {
            data.tw_canary_id = Some(tw_canary_id);
        }

        // Tupperware job handle (format: cluster/user/name)
        if let (Ok(tw_cluster), Ok(tw_user), Ok(tw_name)) = (
            var("TW_JOB_CLUSTER"),
            var("TW_JOB_USER"),
            var("TW_JOB_NAME"),
        ) {
            data.tw_handle = Some(format!("{}/{}/{}", tw_cluster, tw_user, tw_name));

            // Tupperware task handle (format: cluster/user/name/taskid)
            if let Ok(tw_task_id) = var("TW_TASK_ID") {
                data.tw_task_handle = Some(format!(
                    "{}/{}/{}/{}",
                    tw_cluster, tw_user, tw_name, tw_task_id
                ));
            }
        }

        // Chronos fields
        if let Ok(cluster) = var("CHRONOS_CLUSTER") {
            data.chronos_cluster = Some(cluster);
        }
        if let Ok(id) = var("CHRONOS_JOB_INSTANCE_ID") {
            data.chronos_job_instance_id = Some(id);
        }
        if let Ok(job_name) = var("CHRONOS_JOB_NAME") {
            data.chronos_job_name = Some(job_name);
        }

        // Build info
        data.build_revision = Some(build_info::BuildInfo::get_revision().to_string());
        data.build_rule = Some(build_info::BuildInfo::get_rule().to_string());

        data
    }
}

/// Common client/request metadata.
///
/// This struct collects metadata fields that are typically set via
/// `add_metadata()` on Scuba builders. Use `from_metadata()` to gather
/// all available data from a Metadata instance.
#[derive(Debug, Clone, Default)]
pub struct CommonMetadata {
    pub session_uuid: String,
    pub client_identities: Vec<String>,
    pub client_identity_variant: Option<String>,
    pub source_hostname: Option<String>,
    pub client_ip: Option<String>,
    pub unix_username: Option<String>,
    pub sandcastle_alias: Option<String>,
    pub sandcastle_vcs: Option<String>,
    pub sandcastle_nonce: Option<String>,
    pub revproxy_region: Option<String>,
    pub client_tw_job: Option<String>,
    pub client_tw_task: Option<String>,
    pub client_atlas: Option<String>,
    pub client_atlas_env_id: Option<String>,
    pub fetch_cause: Option<String>,
    pub fetch_from_cas_attempted: bool,
    // Client request info fields
    pub client_main_id: Option<String>,
    pub client_entry_point: Option<String>,
    pub client_correlator: Option<String>,
    pub enabled_experiments_jk: Vec<String>,
}

impl CommonMetadata {
    /// Collect common metadata from a Metadata instance.
    pub fn from_metadata(metadata: &Metadata) -> Self {
        let mut data = Self {
            session_uuid: metadata.session_id().to_string(),
            client_identities: metadata
                .identities()
                .iter()
                .map(|i| i.to_string())
                .collect(),
            fetch_from_cas_attempted: metadata.fetch_from_cas_attempted(),
            ..Default::default()
        };

        // Client identity variant
        if let Some(first_identity) = metadata.identities().first() {
            data.client_identity_variant = Some(first_identity.variant().to_string());
        }

        // Source hostname or client IP (mutually exclusive)
        if let Some(client_hostname) = metadata.client_hostname() {
            data.source_hostname = Some(client_hostname.to_owned());
        } else if let Some(client_ip) = metadata.client_ip() {
            data.client_ip = Some(client_ip.to_string());
        }

        // Unix username
        if let Some(unix_name) = metadata.unix_name() {
            data.unix_username = Some(unix_name.to_string());
        }

        // Sandcastle fields
        if let Some(sandcastle_alias) = metadata.sandcastle_alias() {
            data.sandcastle_alias = Some(sandcastle_alias.to_string());
        }
        if let Some(sandcastle_vcs) = metadata.sandcastle_vcs() {
            data.sandcastle_vcs = Some(sandcastle_vcs.to_string());
        }
        if let Some(sandcastle_nonce) = metadata.sandcastle_nonce() {
            data.sandcastle_nonce = Some(sandcastle_nonce.to_string());
        }

        // Reverse proxy region
        if let Some(revproxy_region) = metadata.revproxy_region() {
            data.revproxy_region = Some(revproxy_region.to_string());
        }

        // Tupperware client info
        if let Some(client_tw_job) = metadata.clientinfo_tw_job() {
            data.client_tw_job = Some(client_tw_job.to_string());
        }
        if let Some(client_tw_task) = metadata.clientinfo_tw_task() {
            data.client_tw_task = Some(client_tw_task.to_string());
        }

        // Atlas client info
        if let Some(client_atlas) = metadata.clientinfo_atlas() {
            data.client_atlas = Some(client_atlas.to_string());
        }
        if let Some(client_atlas_env_id) = metadata.clientinfo_atlas_env_id() {
            data.client_atlas_env_id = Some(client_atlas_env_id.to_string());
        }

        // Fetch fields
        if let Some(fetch_cause) = metadata.fetch_cause() {
            data.fetch_cause = Some(fetch_cause.to_string());
        }

        // Client request info
        if let Some(cri) = metadata.client_request_info() {
            if let Some(main_id) = &cri.main_id {
                data.client_main_id = Some(main_id.clone());
            }
            data.client_entry_point = Some(cri.entry_point.to_string());
            data.client_correlator = Some(cri.correlator.clone());
            data.enabled_experiments_jk =
                MononokeScubaSampleBuilder::get_enabled_experiments_jk(cri);
        }

        data
    }
}
