/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! RIM-backed rate limiting on the EdenAPI request path. Supports both comparison
//! against the legacy rate limiter and authoritative enforcement. RIM failures
//! always fail open.
//!
//! Configerator source of truth: `source/rim/backend_settings/mononoke_server/`.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::Result;
use backend_if::RimBackend;
use cached_config::ConfigHandle;
use cached_config::ConfigStore;
use context::CoreContext;
use metadata::RimTenancyPath;
use permission_checker::TenantInfo;
use rate_limiting_config::RimWorkloadConfig;
use rim_ligen::RimThinClient;
use tokio::time::timeout;
use tracing::debug;
use tracing::warn;

const RIM_ACQUIRE_TIMEOUT: Duration = Duration::from_millis(500);
const WORKLOAD_CONFIG_PATH: &str = "scm/mononoke/ratelimiting/rim_workloads";

pub(crate) type WorkloadConfig = ConfigHandle<RimWorkloadConfig>;

pub(crate) fn workload_config(store: &ConfigStore) -> Result<WorkloadConfig> {
    store.get_config_handle(WORKLOAD_CONFIG_PATH.to_owned())
}

pub(crate) fn workload_tenancy_path(
    path: RimTenancyPath,
    config: &RimWorkloadConfig,
    backend: RimBackend,
) -> Vec<String> {
    if !config.allocation_backends.contains(&backend.0) {
        return path.into_vec();
    }
    path.with_workload_allocation(|category, workload| {
        config
            .named_workloads
            .get(category)
            .is_some_and(|workloads| workloads.contains(workload))
    })
    .into_vec()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RimDecision {
    Allow,
    Reject,
    FailOpen,
}

pub(crate) fn legacy_tenancy_path(
    tenant: &TenantInfo,
    region: Option<&str>,
) -> Option<Vec<String>> {
    let mut path = tenant.legacy_tenancy_path()?;
    if let Some(region) = region {
        path.insert(2, region.to_owned());
    }
    Some(path)
}

/// Call once at server startup. Failures are logged and swallowed so RIM
/// infra outages can't prevent the server from serving traffic.
pub(crate) fn init(rim_backend: RimBackend) {
    match RimThinClient::initialize(rim_backend) {
        Ok(status) if status.success() => {
            debug!("RIM thin client initialized for backend {:?}", rim_backend);
        }
        Ok(status) => {
            warn!(
                "RIM initialize returned non-success: code={:?} msg={}",
                status.code(),
                status.message(),
            );
        }
        Err(e) => {
            warn!("RIM initialize failed for backend {:?}: {}", rim_backend, e);
        }
    }
}

pub(crate) async fn check_rate_limit(
    ctx: &CoreContext,
    tenancy_path: &[String],
    rim_backend: RimBackend,
    requirements: &HashMap<String, f64>,
) -> RimDecision {
    let log = |tag: &str, detail: String| {
        let mut scuba = ctx.scuba().clone();
        scuba.add("rim_tenancy_path", tenancy_path.join("/"));
        scuba.add("rim_backend", rim_backend.0);
        scuba.add("rim_requirements", format!("{requirements:?}"));
        scuba.log_with_msg(tag, detail);
    };

    let result = match timeout(
        RIM_ACQUIRE_TIMEOUT,
        RimThinClient::acquire(rim_backend, tenancy_path.to_vec(), requirements.clone()),
    )
    .await
    {
        Ok(Ok(result)) => result,
        Ok(Err(e)) => {
            log("RIM rate-limit check error", e.to_string());
            return RimDecision::FailOpen;
        }
        Err(_) => {
            log(
                "RIM rate-limit check timeout",
                format!("timeout after {RIM_ACQUIRE_TIMEOUT:?}"),
            );
            return RimDecision::FailOpen;
        }
    };

    if result.rejected() {
        log("RIM rejected request", format!("code={:?}", result.code()));
        RimDecision::Reject
    } else if result.failed() {
        log(
            "RIM rate-limit check failed",
            format!("code={:?}", result.code()),
        );
        RimDecision::FailOpen
    } else {
        RimDecision::Allow
    }
}

pub(crate) async fn report_load(
    ctx: &CoreContext,
    tenancy_path: &[String],
    rim_backend: RimBackend,
    usage: &HashMap<String, f64>,
) {
    let log = |tag: &str, detail: String| {
        let mut scuba = ctx.scuba().clone();
        scuba.add("rim_tenancy_path", tenancy_path.join("/"));
        scuba.add("rim_backend", rim_backend.0);
        scuba.add("rim_requirements", format!("{usage:?}"));
        scuba.log_with_msg(tag, detail);
    };

    match timeout(
        RIM_ACQUIRE_TIMEOUT,
        RimThinClient::report(rim_backend, tenancy_path.to_vec(), usage.clone()),
    )
    .await
    {
        Ok(Ok(result)) if !result.success() => {
            log("RIM report non-success", format!("{:?}", result.status()));
        }
        Ok(Ok(_)) => {}
        Ok(Err(e)) => {
            log("RIM report error", e.to_string());
        }
        Err(_) => {
            log(
                "RIM report timeout",
                format!("timeout after {RIM_ACQUIRE_TIMEOUT:?}"),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use backend_if::RimBackend;
    use cached_config::ConfigStore;
    use cached_config::ModificationTime;
    use cached_config::TestSource;
    use metadata::Metadata;
    use mononoke_macros::mononoke;
    use permission_checker::TenantInfo;
    use rate_limiting_config::RimWorkloadConfig;

    use super::WORKLOAD_CONFIG_PATH;
    use super::legacy_tenancy_path;
    use super::workload_config;
    use super::workload_tenancy_path;

    #[mononoke::test]
    fn allocation_follows_live_catalog() {
        let source = Arc::new(TestSource::new());
        let store = ConfigStore::new(source.clone(), None::<Duration>, None);
        let mut handle = None;
        for (version, grouped, named) in [
            (1, false, false),
            (2, true, false),
            (3, true, true),
            (4, true, false),
            (5, false, false),
        ] {
            let config = RimWorkloadConfig {
                allocation_backends: if grouped {
                    [RimBackend::RIM_USE_CASE_52.0, RimBackend::RIM_USE_CASE_79.0].into()
                } else {
                    Default::default()
                },
                named_workloads: if named {
                    [("unknown".to_owned(), ["unknown".to_owned()].into())].into()
                } else {
                    Default::default()
                },
            };
            source.insert_config(
                WORKLOAD_CONFIG_PATH,
                core::str::from_utf8(&fbthrift::simplejson_protocol::serialize(&config)).unwrap(),
                ModificationTime::UnixTimestamp(version),
            );
            source.insert_to_refresh(WORKLOAD_CONFIG_PATH.to_owned());
            let handle = handle.get_or_insert_with(|| workload_config(&store).unwrap());
            store.force_update_configs();
            for (backend, region) in [
                (RimBackend::RIM_USE_CASE_52, None),
                (RimBackend::RIM_USE_CASE_79, Some("atn")),
                (RimBackend::RIM_USE_CASE_79, Some("ldc")),
                (RimBackend::MONONOKE_SERVER, None),
            ] {
                let path = Metadata::default().rim_tenancy_path(region);
                let expected: Vec<_> = std::iter::once("root")
                    .chain(region)
                    .chain(["unknown"])
                    .chain(
                        (grouped && backend != RimBackend::MONONOKE_SERVER).then_some(if named {
                            "unknown"
                        } else {
                            "__mononoke_remaining__"
                        }),
                    )
                    .chain(["unknown", "default"])
                    .collect();
                assert_eq!(
                    workload_tenancy_path(path, &handle.get(), backend),
                    expected
                );
            }
            source.insert_config(
                WORKLOAD_CONFIG_PATH,
                "invalid",
                ModificationTime::UnixTimestamp(100 + version),
            );
            source.insert_to_refresh(WORKLOAD_CONFIG_PATH.to_owned());
            store.force_update_configs();
            assert_eq!(*handle.get(), config);
        }
    }

    #[mononoke::test]
    fn workload_catalog_is_required_at_startup() {
        let source = Arc::new(TestSource::new());
        let store = ConfigStore::new(source.clone(), None::<Duration>, None);
        assert!(workload_config(&store).is_err());

        source.insert_config(
            WORKLOAD_CONFIG_PATH,
            "invalid",
            ModificationTime::UnixTimestamp(1),
        );
        assert!(workload_config(&store).is_err());

        let config = RimWorkloadConfig::default();
        source.insert_config(
            WORKLOAD_CONFIG_PATH,
            core::str::from_utf8(&fbthrift::simplejson_protocol::serialize(&config)).unwrap(),
            ModificationTime::UnixTimestamp(2),
        );
        let handle = workload_config(&store).unwrap();
        assert_eq!(*handle.get(), config);
    }

    #[mononoke::test]
    fn global_and_regional_paths_preserve_category_and_client() {
        let tenant = TenantInfo::DevEnv {
            client_id: Some("MACHINE:devserver.example.com".to_owned()),
        };
        assert_eq!(
            legacy_tenancy_path(&tenant, None),
            Some(vec![
                "root".to_owned(),
                "dev_env".to_owned(),
                "MACHINE:devserver.example.com".to_owned()
            ]),
        );
        assert_eq!(
            legacy_tenancy_path(&tenant, Some("ldc")),
            Some(vec![
                "root".to_owned(),
                "dev_env".to_owned(),
                "ldc".to_owned(),
                "MACHINE:devserver.example.com".to_owned()
            ]),
        );
    }

    #[mononoke::test]
    fn missing_client_has_no_global_or_regional_path() {
        let tenant = TenantInfo::Unknown { client_id: None };
        assert_eq!(legacy_tenancy_path(&tenant, None), None);
        assert_eq!(legacy_tenancy_path(&tenant, Some("ldc")), None);
    }
}
