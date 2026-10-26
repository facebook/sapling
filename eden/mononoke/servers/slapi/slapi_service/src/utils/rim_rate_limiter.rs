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
//! Configerator source of truth: `source/rim/backend_settings/mononoke_workload/`.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::Result;
use anyhow::ensure;
use backend_if::RimBackend;
use cached_config::ConfigHandle;
use cached_config::ConfigStore;
use context::CoreContext;
use metadata::Metadata;
use permission_checker::MononokeIdentitySetExt;
use permission_checker::TenantInfo;
use rate_limiting_config::RimMetadataField;
use rate_limiting_config::RimWorkloadConfig;
use rim_if::ReturnCode;
use rim_ligen::RimThinClient;
use sha2::Digest;
use sha2::Sha256;
use stats::prelude::*;
use tokio::time::timeout;
use tracing::debug;
use tracing::warn;

const RIM_ACQUIRE_TIMEOUT: Duration = Duration::from_millis(500);
const WORKLOAD_CONFIG_PATH: &str = "scm/mononoke/ratelimiting/rim_workloads";

define_stats! {
    prefix = "mononoke.rim";
    admission_report_errors: timeseries(Rate, Sum),
}

pub(crate) type WorkloadConfig = ConfigHandle<RimWorkloadConfig>;

pub(crate) fn workload_config(store: &ConfigStore) -> Result<WorkloadConfig> {
    store.get_config_handle(WORKLOAD_CONFIG_PATH.to_owned())
}

pub(crate) fn request_attribution(
    metadata: &Metadata,
    region: Option<&str>,
    config: &RimWorkloadConfig,
    backend: RimBackend,
) -> (Vec<String>, Option<String>) {
    let path = metadata.rim_tenancy_path(region);
    if !config.allocation_backends.contains(&backend.0) {
        return (path.into_vec(), None);
    }
    let selection_key = config
        .unit_of_work
        .get(path.category().as_str())
        .and_then(|field| metadata_field(metadata, *field));
    let path = path.with_workload_allocation(|category, workload| {
        config
            .named_workloads
            .get(category)
            .is_some_and(|workloads| workloads.contains(workload))
    });
    (path.into_vec(), selection_key)
}

fn metadata_field(metadata: &Metadata, field: RimMetadataField) -> Option<String> {
    let (prefix, value) = match field {
        RimMetadataField::SANDCASTLE_JOB => {
            ("SANDCASTLE_JOB", metadata.identities().sandcastle_job_id())
        }
        _ => return None,
    };
    value
        .filter(|value| !value.is_empty())
        .map(|value| format!("{prefix}:{value}"))
}

#[derive(Clone, Copy)]
pub(crate) enum FetchResource {
    Files,
    Trees,
}

pub(crate) fn request_requirements(
    config: Option<&RimWorkloadConfig>,
    backend: RimBackend,
    fetches: Option<(FetchResource, u32)>,
) -> HashMap<String, f64> {
    let cost = config
        .filter(|config| config.allocation_backends.contains(&backend.0))
        .and_then(|config| config.request_cost.as_ref());
    if let Some(cost) = cost {
        let fetch_cost = match fetches {
            Some((FetchResource::Files, count)) => f64::from(cost.file) * f64::from(count),
            Some((FetchResource::Trees, count)) => f64::from(cost.tree) * f64::from(count),
            None => 0.0,
        };
        return [("cost".to_owned(), f64::from(cost.request) + fetch_cost)].into();
    }
    let fetches = fetches.map(|(resource, count)| {
        let resource = match resource {
            FetchResource::Files => "file_fetches",
            FetchResource::Trees => "tree_fetches",
        };
        (resource, f64::from(count))
    });
    [Some(("qps", 1.0)), fetches]
        .into_iter()
        .flatten()
        .map(|(resource, units)| (resource.to_owned(), units))
        .collect()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RimDecision {
    Allow,
    Reject,
    FailOpen,
}

fn unit_decision(backend: RimBackend, key: &str, admit_percent: f64) -> RimDecision {
    if !admit_percent.is_finite() || !(0.0..=100.0).contains(&admit_percent) {
        return RimDecision::FailOpen;
    }
    let hash = Sha256::new()
        .chain_update(backend.0.to_be_bytes())
        .chain_update(key.as_bytes())
        .finalize();
    let mut bytes = [0; 8];
    bytes.copy_from_slice(&hash[..8]);
    let rank = (u64::from_be_bytes(bytes) >> 11) as f64 / (1u64 << 53) as f64;
    if rank * 100.0 < admit_percent {
        RimDecision::Allow
    } else {
        RimDecision::Reject
    }
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
    selection_key: Option<&str>,
) -> RimDecision {
    let log = |tag: &str, detail: String| {
        let mut scuba = ctx.scuba().clone();
        scuba.add("rim_tenancy_path", tenancy_path.join("/"));
        scuba.add("rim_backend", rim_backend.0);
        scuba.add("rim_requirements", format!("{requirements:?}"));
        scuba.log_with_msg(tag, detail);
    };

    let result = match timeout(RIM_ACQUIRE_TIMEOUT, async {
        let options = rim_ligen::create_acquire_options(selection_key.is_none())?;
        RimThinClient::acquire_v2(
            rim_backend,
            tenancy_path.to_vec(),
            requirements.clone(),
            options,
        )
        .await
    })
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

    if result.failed() && !result.rejected() {
        log(
            "RIM rate-limit check failed",
            format!("code={:?}", result.code()),
        );
        return RimDecision::FailOpen;
    }

    let decision = if let Some(key) = selection_key {
        let decision = unit_decision(rim_backend, key, result.admit_probability());
        if decision == RimDecision::FailOpen {
            log(
                "RIM rate-limit check failed",
                format!(
                    "invalid admission percentage: {}",
                    result.admit_probability()
                ),
            );
            return decision;
        }
        if let Err(error) = report_admission(
            rim_backend,
            tenancy_path,
            requirements,
            decision == RimDecision::Reject,
        ) {
            STATS::admission_report_errors.add_value(1);
            log("RIM admission report failed", error.to_string());
        }
        decision
    } else if result.rejected() {
        RimDecision::Reject
    } else {
        RimDecision::Allow
    };
    if decision == RimDecision::Reject {
        log(
            "RIM rejected request",
            format!("admission percentage={}", result.admit_probability()),
        );
    }
    decision
}

fn report_admission(
    backend: RimBackend,
    path: &[String],
    requirements: &HashMap<String, f64>,
    rejected: bool,
) -> Result<()> {
    let code = if rejected {
        ReturnCode::ADMITTANCE_REJECTED
    } else {
        ReturnCode::SUCCESS
    };
    let status = RimThinClient::track_resource_request_explicitly(
        backend,
        path.to_vec(),
        requirements
            .keys()
            .map(|resource| (resource.clone(), (code, 1)))
            .collect(),
    )?;
    ensure!(status.success(), "code={:?}", status.code());
    Ok(())
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
    use clientinfo::ClientInfo;
    use metadata::Metadata;
    use mononoke_macros::mononoke;
    use permission_checker::MononokeIdentity;
    use permission_checker::TenantInfo;
    use rate_limiting_config::RimMetadataField;
    use rate_limiting_config::RimRequestCost;
    use rate_limiting_config::RimWorkloadConfig;

    use super::FetchResource;
    use super::RimDecision;
    use super::WORKLOAD_CONFIG_PATH;
    use super::legacy_tenancy_path;
    use super::request_attribution;
    use super::request_requirements;
    use super::unit_decision;
    use super::workload_config;

    #[mononoke::test]
    fn unit_selection_respects_cutoffs_and_rejects_invalid_percentages() {
        let backend = RimBackend::RIM_USE_CASE_52;
        let key = "SANDCASTLE_JOB:123";
        assert_eq!(unit_decision(backend, key, 0.0), RimDecision::Reject);
        assert_eq!(unit_decision(backend, key, 100.0), RimDecision::Allow);
        for invalid in [-1.0, 101.0, f64::NAN, f64::INFINITY] {
            assert_eq!(unit_decision(backend, key, invalid), RimDecision::FailOpen);
        }
    }

    #[mononoke::test]
    fn unit_selection_is_nested_and_backend_domains_are_independent() {
        let mut global_admitted = 0;
        let mut both_admitted = 0;
        for job in 0..2000 {
            let key = format!("SANDCASTLE_JOB:{job}");
            let global = unit_decision(RimBackend::RIM_USE_CASE_52, &key, 50.0);
            let regional = unit_decision(RimBackend::RIM_USE_CASE_79, &key, 50.0);
            if global == RimDecision::Allow {
                global_admitted += 1;
                assert_eq!(
                    unit_decision(RimBackend::RIM_USE_CASE_52, &key, 80.0),
                    RimDecision::Allow,
                );
                if regional == RimDecision::Allow {
                    both_admitted += 1;
                }
            } else {
                assert_eq!(
                    unit_decision(RimBackend::RIM_USE_CASE_52, &key, 20.0),
                    RimDecision::Reject,
                );
            }
        }
        assert!((850..1150).contains(&global_admitted));
        assert!((350..650).contains(&both_admitted));
    }

    fn ci_metadata(job: Option<&str>, purpose: Option<&str>) -> Metadata {
        let mut metadata = Metadata::default().set_identities(
            std::iter::once(MononokeIdentity::from_legacy_type_data(
                "SERVICE_IDENTITY",
                "sandcastle.svn.push",
            ))
            .chain(job.map(|job| MononokeIdentity::from_legacy_type_data("SANDCASTLE_JOB", job)))
            .collect(),
        );
        metadata.add_client_info(
            ClientInfo::from_json(
                &serde_json::json!({"sandcastle_alias": "build", "ci_purpose": purpose})
                    .to_string(),
            )
            .unwrap(),
        );
        metadata
    }

    fn unit_config() -> RimWorkloadConfig {
        RimWorkloadConfig {
            allocation_backends: [RimBackend::RIM_USE_CASE_52.0, RimBackend::RIM_USE_CASE_79.0]
                .into(),
            unit_of_work: [("ci_sandcastle".to_owned(), RimMetadataField::SANDCASTLE_JOB)].into(),
            named_workloads: [(
                "ci_sandcastle".to_owned(),
                ["CI_PURPOSE:land".to_owned()].into(),
            )]
            .into(),
            ..Default::default()
        }
    }

    #[mononoke::test]
    fn ci_attempts_share_a_purpose_without_splitting_fetch_causes() {
        let config = unit_config();
        for (backend, region) in [
            (RimBackend::RIM_USE_CASE_52, None),
            (RimBackend::RIM_USE_CASE_79, Some("atn")),
        ] {
            for job in ["123", "456"] {
                for cause in [
                    None,
                    Some("sl-prefetch"),
                    Some("edenfs-prefetch"),
                    Some("eden-walk-prefetch"),
                    Some("sl-checkout"),
                ] {
                    let metadata = ci_metadata(Some(job), Some("land"))
                        .set_fetch_cause(cause.map(str::to_owned));
                    let (path, key) = request_attribution(&metadata, region, &config, backend);
                    let expected: Vec<_> = std::iter::once("root")
                        .chain(region)
                        .chain([
                            "ci_sandcastle",
                            "CI_PURPOSE:land",
                            "CI_PURPOSE:land",
                            "default",
                        ])
                        .collect();
                    assert_eq!(path, expected);
                    assert_eq!(key, Some(format!("SANDCASTLE_JOB:{job}")));
                }
            }
        }
    }

    #[mononoke::test]
    fn unit_selection_requires_both_config_and_a_unit_id() {
        let mut config = unit_config();
        let backend = RimBackend::RIM_USE_CASE_52;
        for job in [None, Some("")] {
            assert_eq!(
                request_attribution(&ci_metadata(job, Some("land")), None, &config, backend).1,
                None
            );
        }
        let metadata = ci_metadata(Some("123"), Some("land"));
        assert_eq!(
            request_attribution(&metadata, None, &config, RimBackend::MONONOKE_SERVER),
            (metadata.rim_tenancy_path(None).into_vec(), None)
        );
        let interactive = Metadata::default()
            .set_identities([MononokeIdentity::from_legacy_type_data("USER", "alice")].into());
        assert_eq!(
            request_attribution(&interactive, None, &config, backend).1,
            None
        );
        for purpose in [None, Some(""), Some("new_purpose")] {
            let (path, _) =
                request_attribution(&ci_metadata(Some("123"), purpose), None, &config, backend);
            assert_eq!(path[2], "__mononoke_remaining__");
            assert_eq!(
                path[3],
                purpose
                    .filter(|p| !p.is_empty())
                    .map(|p| format!("CI_PURPOSE:{p}"))
                    .unwrap_or_else(|| "SERVICE_IDENTITY:sandcastle.svn.push".to_owned())
            );
        }
        let warming = Metadata::default().set_identities(
            [MononokeIdentity::from_legacy_type_data(
                "SERVICE_IDENTITY",
                "sandcastle.worker.warmed",
            )]
            .into(),
        );
        let (path, key) = request_attribution(&warming, None, &config, backend);
        assert_eq!(path[1], "sandcastle_automation");
        assert_eq!(key, None);
        config.unit_of_work.clear();
        let metadata = metadata.set_fetch_cause(Some("sl-prefetch".to_owned()));
        let (path, key) = request_attribution(&metadata, None, &config, backend);
        assert_eq!(key, None);
        assert_eq!(
            path,
            vec![
                "root",
                "ci_sandcastle",
                "CI_PURPOSE:land",
                "CI_PURPOSE:land",
                "default",
            ]
        );
    }

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
                request_cost: Some(RimRequestCost {
                    request: version as i32,
                    file: 2,
                    tree: 1,
                }),
                unit_of_work: Default::default(),
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
                let requirements = request_requirements(Some(&handle.get()), backend, None);
                let expected_requirements = if grouped && backend != RimBackend::MONONOKE_SERVER {
                    [("cost".to_owned(), version as f64)].into()
                } else {
                    [("qps".to_owned(), 1.0)].into()
                };
                assert_eq!(requirements, expected_requirements);
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
                    request_attribution(&Metadata::default(), region, &handle.get(), backend).0,
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
    fn request_cost_accounts_for_batches_and_preserves_legacy_backends() {
        let mut config = RimWorkloadConfig {
            allocation_backends: [RimBackend::RIM_USE_CASE_52.0, RimBackend::RIM_USE_CASE_79.0]
                .into(),
            request_cost: Some(RimRequestCost {
                request: 14,
                file: 2,
                tree: 1,
            }),
            ..Default::default()
        };
        for backend in [RimBackend::RIM_USE_CASE_52, RimBackend::RIM_USE_CASE_79] {
            for (fetches, expected) in [
                (None, 14.0),
                (Some((FetchResource::Files, 0)), 14.0),
                (Some((FetchResource::Files, 4000)), 8014.0),
                (Some((FetchResource::Trees, 4000)), 4014.0),
                (Some((FetchResource::Files, u32::MAX)), 8589934604.0),
            ] {
                assert_eq!(
                    request_requirements(Some(&config), backend, fetches),
                    [("cost".to_owned(), expected)].into(),
                );
            }
        }
        let fetches = Some((FetchResource::Files, 4000));
        let legacy = [("qps".to_owned(), 1.0), ("file_fetches".to_owned(), 4000.0)].into();
        assert_eq!(
            request_requirements(None, RimBackend::RIM_USE_CASE_52, fetches),
            legacy
        );
        assert_eq!(
            request_requirements(Some(&config), RimBackend::MONONOKE_SERVER, fetches),
            legacy
        );
        config.request_cost = None;
        assert_eq!(
            request_requirements(Some(&config), RimBackend::RIM_USE_CASE_52, fetches),
            legacy
        );
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
