/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
#[cfg(fbcode_build)]
use authenticated_identity_thrift::AuthenticatedIdentity;
#[cfg(fbcode_build)]
use infrasec_authorization::Identity as ThriftIdentity;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;

#[cfg(not(fbcode_build))]
use crate::oss::AuthenticatedIdentity;
#[cfg(not(fbcode_build))]
use crate::oss::Identity as OssIdentity;

pub type MononokeIdentitySet = BTreeSet<MononokeIdentity>;

/// Newtype wrapper around `AuthenticatedIdentity`. All Mononoke identities are
/// `AuthenticatedIdentity`s now -- "thin" ones produced by
/// `MononokeIdentity::from_legacy_type_data` carry only `id_type` / `id_data` with
/// empty attributes, while ingress paths (mTLS cert, `mid://` SAN URIs, forwarded
/// JSON envelope, srserver) populate the full struct.
///
/// The inner `AuthenticatedIdentity` is private to keep ingestion centralized:
/// construct via `MononokeIdentity::from(auth_id)` (or
/// `MononokeIdentity::from_legacy_type_data(...)`), and access via
/// `inner()` / `into_inner()` / the `id_type()` / `id_data()` accessors.
#[derive(Clone, Debug)]
pub struct MononokeIdentity(AuthenticatedIdentity);

// Manual implementations for Eq, PartialEq, Hash, Ord, PartialOrd
// that compare based on id_type and id_data only -- attributes/source/etc
// are not part of identity equality (an identity with different attributes
// is still the same identity).
impl PartialEq for MononokeIdentity {
    fn eq(&self, other: &Self) -> bool {
        self.id_type() == other.id_type() && self.id_data() == other.id_data()
    }
}

impl Eq for MononokeIdentity {}

impl std::hash::Hash for MononokeIdentity {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id_type().hash(state);
        self.id_data().hash(state);
    }
}

impl Ord for MononokeIdentity {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.id_type(), self.id_data()).cmp(&(other.id_type(), other.id_data()))
    }
}

impl PartialOrd for MononokeIdentity {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl MononokeIdentity {
    /// Construct a "thin" identity from a legacy `(id_type, id_data)` pair.
    ///
    /// The result is a `MononokeIdentity` wrapping an `AuthenticatedIdentity` whose
    /// only populated fields are `identity.id_type` / `identity.id_data` -- empty
    /// `attributes`, empty `loggingKey`, no `catPayload`, default `Source::UNKNOWN`.
    /// All the rich metadata that a real credential would carry (agent attributes,
    /// origin tags, transport source, CAT payload, etc.) is lost.
    ///
    /// **Prefer wrapping a full `AuthenticatedIdentity` whenever the caller actually
    /// has one** -- use `MononokeIdentity::from(auth_id)` (or `MononokeIdentity(auth_id)`)
    /// for credentials parsed via `try_from_x509` / `try_from_json_encoded` /
    /// `authenticated_identities_struct()` / CAT verification. This function should be
    /// reserved for cases where only `(id_type, id_data)` is available -- synthetic
    /// identities (allowlist entries from configerator, reviewer identities, hook
    /// author lookups by unixname, test fixtures, the OSS `X509_SUBJECT_NAME`
    /// fallback). Such identities are not distinguishable from real ones at the
    /// `MononokeIdentitySet` level, but downstream code that inspects `attributes`
    /// or `source` will see empty / default values.
    pub fn from_legacy_type_data(id_type: impl Into<String>, id_data: impl Into<String>) -> Self {
        let id_type = id_type.into();
        let id_data = id_data.into();
        #[cfg(fbcode_build)]
        let auth_id = AuthenticatedIdentity {
            identity: ThriftIdentity {
                id_type,
                id_data,
                ..Default::default()
            },
            ..Default::default()
        };
        #[cfg(not(fbcode_build))]
        let auth_id = AuthenticatedIdentity {
            identity: OssIdentity { id_type, id_data },
            attributes: vec![],
        };
        Self(auth_id)
    }

    pub fn id_type(&self) -> &str {
        self.0.identity.id_type.as_str()
    }

    pub fn id_data(&self) -> &str {
        self.0.identity.id_data.as_str()
    }

    pub fn is_of_type(&self, id_type: &str) -> bool {
        self.id_type() == id_type
    }

    /// Render the identity in the debug-friendly form produced by the C++
    /// canonical logging formatter at `access/if/AuthenticatedIdentity.cpp`:
    /// `AuthenticatedIdentity{identity=TYPE:data, source=ENUM, attributes=[{ns/name=val}, ...]}`.
    ///
    /// Used for Scuba's `client_identities_typed` column and for log lines
    /// that surface client identity. Attributes are emitted without URI
    /// escaping, structure is explicit, and `source` is included as a name --
    /// so this is suitable for human reading and Scuba grouping, but **not**
    /// for wire envelopes or anything that needs to round-trip through a URI
    /// parser. For those, serialize the underlying `AuthenticatedIdentity` via
    /// `authenticated_identity_serializer::serialize` directly.
    ///
    /// In OSS builds the C++ formatter is unavailable, so this falls back to
    /// a plain `TYPE:data` summary that drops attributes.
    pub fn to_typed_string(&self) -> String {
        #[cfg(fbcode_build)]
        {
            authenticated_identity_serializer::to_string(self.0.clone())
        }
        #[cfg(not(fbcode_build))]
        {
            format!("{}:{}", self.id_type(), self.id_data())
        }
    }

    /// Borrow the inner `AuthenticatedIdentity`.
    pub fn inner(&self) -> &AuthenticatedIdentity {
        &self.0
    }

    /// Consume the wrapper and return the inner `AuthenticatedIdentity`.
    pub fn into_inner(self) -> AuthenticatedIdentity {
        self.0
    }
}

impl From<AuthenticatedIdentity> for MononokeIdentity {
    fn from(auth_id: AuthenticatedIdentity) -> Self {
        Self(auth_id)
    }
}

impl fmt::Display for MononokeIdentity {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(fmt, "{}:{}", self.id_type(), self.id_data())
    }
}

impl FromStr for MononokeIdentity {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (ty, data) = value.split_once(':').with_context(|| {
            format!("MononokeIdentity parse error, expected TYPE:data, got {value:?}")
        })?;
        Ok(Self::from_legacy_type_data(ty, data))
    }
}

impl Serialize for MononokeIdentity {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for MononokeIdentity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_str(&s).map_err(serde::de::Error::custom)
    }
}

pub trait MononokeIdentitySetExt {
    fn likely_an_agent(&self) -> bool;

    fn is_proxygen_test_identity(&self) -> bool;

    fn hostprefix(&self) -> Option<&str>;

    fn hostname(&self) -> Option<&str>;

    fn username(&self) -> Option<&str>;
    fn crewmate(&self) -> Option<&str>;
    fn sandcastle_job_id(&self) -> Option<&str>;
    fn on_demand_type(&self) -> Option<&str>;

    fn identity_type_filtered_concat(&self, id_type: &str) -> Option<String>;
    fn main_client_identity(&self, sandcastle_alias: Option<&str>) -> String;

    /// Classify this identity set into a coarse [`ClientCategory`] for
    /// rate-limit policy and Scuba logging. Sandcastle traffic without an
    /// alias is classified separately from CI traffic with an alias.
    fn client_category(&self, sandcastle_alias: Option<&str>) -> ClientCategory;

    fn to_string(&self) -> String;
}

/// Coarse client categories derived from the identity set.
///
/// Used as a per-request Scuba dimension and (eventually) the key for
/// per-category rate-limit allowances. Rules live in the `fbcode_build`
/// impl of [`MononokeIdentitySetExt::client_category`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClientCategory {
    HealthCheck,
    InteractiveDev,
    DevEnv,
    CiSandcastle,
    SandcastleAutomation,
    Mast,
    FaaS,
    Automation,
    Unknown,
}

impl ClientCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::HealthCheck => "health_check",
            Self::InteractiveDev => "interactive_dev",
            Self::DevEnv => "dev_env",
            Self::CiSandcastle => "ci_sandcastle",
            Self::SandcastleAutomation => "sandcastle_automation",
            Self::Mast => "mast",
            Self::FaaS => "faas",
            Self::Automation => "automation",
            Self::Unknown => "unknown",
        }
    }
}

/// A request's tenancy dimensions, used for RIM attribution and rate-limit
/// policy.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TenantInfo {
    HealthCheck {
        client_id: Option<String>,
    },
    InteractiveDev {
        client_id: Option<String>,
    },
    DevEnv {
        client_id: Option<String>,
        on_demand_type: Option<String>,
        client_region: Option<String>,
        client_hostname: Option<String>,
    },
    CiSandcastle {
        client_id: Option<String>,
        ci_purpose: Option<String>,
        sandcastle_job_id: Option<String>,
    },
    SandcastleAutomation {
        client_id: Option<String>,
    },
    Mast {
        client_id: Option<String>,
        data_project: Option<String>,
        offline_job_root_run_id: Option<String>,
        offline_job_leaf_run_id: Option<String>,
    },
    FaaS {
        client_id: Option<String>,
        atlas_env_id: Option<String>,
        atlas_rl: Option<bool>,
        atlas_purpose: Option<String>,
        faas_job_name: Option<String>,
    },
    Automation {
        client_id: Option<String>,
    },
    Unknown {
        client_id: Option<String>,
    },
}

impl TenantInfo {
    pub fn category(&self) -> ClientCategory {
        match self {
            Self::HealthCheck { .. } => ClientCategory::HealthCheck,
            Self::InteractiveDev { .. } => ClientCategory::InteractiveDev,
            Self::DevEnv { .. } => ClientCategory::DevEnv,
            Self::CiSandcastle { .. } => ClientCategory::CiSandcastle,
            Self::SandcastleAutomation { .. } => ClientCategory::SandcastleAutomation,
            Self::Mast { .. } => ClientCategory::Mast,
            Self::FaaS { .. } => ClientCategory::FaaS,
            Self::Automation { .. } => ClientCategory::Automation,
            Self::Unknown { .. } => ClientCategory::Unknown,
        }
    }

    pub fn client_id(&self) -> Option<&str> {
        match self {
            Self::HealthCheck { client_id }
            | Self::InteractiveDev { client_id }
            | Self::DevEnv { client_id, .. }
            | Self::CiSandcastle { client_id, .. }
            | Self::SandcastleAutomation { client_id }
            | Self::Mast { client_id, .. }
            | Self::FaaS { client_id, .. }
            | Self::Automation { client_id }
            | Self::Unknown { client_id } => client_id.as_deref(),
        }
    }

    pub fn ci_purpose(&self) -> Option<&str> {
        match self {
            Self::CiSandcastle { ci_purpose, .. } => ci_purpose.as_deref(),
            _ => None,
        }
    }

    pub fn sandcastle_job_id(&self) -> Option<&str> {
        match self {
            Self::CiSandcastle {
                sandcastle_job_id, ..
            } => sandcastle_job_id.as_deref(),
            _ => None,
        }
    }

    pub fn on_demand_type(&self) -> Option<&str> {
        match self {
            Self::DevEnv { on_demand_type, .. } => on_demand_type.as_deref(),
            _ => None,
        }
    }

    pub fn client_region(&self) -> Option<&str> {
        match self {
            Self::DevEnv { client_region, .. } => client_region.as_deref(),
            _ => None,
        }
    }

    pub fn client_hostname(&self) -> Option<&str> {
        match self {
            Self::DevEnv {
                client_hostname, ..
            } => client_hostname.as_deref(),
            _ => None,
        }
    }

    pub fn data_project(&self) -> Option<&str> {
        match self {
            Self::Mast { data_project, .. } => data_project.as_deref(),
            _ => None,
        }
    }

    pub fn offline_job_root_run_id(&self) -> Option<&str> {
        match self {
            Self::Mast {
                offline_job_root_run_id,
                ..
            } => offline_job_root_run_id.as_deref(),
            _ => None,
        }
    }

    pub fn offline_job_leaf_run_id(&self) -> Option<&str> {
        match self {
            Self::Mast {
                offline_job_leaf_run_id,
                ..
            } => offline_job_leaf_run_id.as_deref(),
            _ => None,
        }
    }

    pub fn atlas_env_id(&self) -> Option<&str> {
        match self {
            Self::FaaS { atlas_env_id, .. } => atlas_env_id.as_deref(),
            _ => None,
        }
    }

    pub fn atlas_rl(&self) -> Option<bool> {
        match self {
            Self::FaaS { atlas_rl, .. } => *atlas_rl,
            _ => None,
        }
    }

    pub fn atlas_purpose(&self) -> Option<&str> {
        match self {
            Self::FaaS { atlas_purpose, .. } => atlas_purpose.as_deref(),
            _ => None,
        }
    }

    pub fn faas_job_name(&self) -> Option<&str> {
        match self {
            Self::FaaS { faas_job_name, .. } => faas_job_name.as_deref(),
            _ => None,
        }
    }

    /// RIM tenancy hierarchy path: root -> category -> client id. `None` when
    /// there is no `client_id` to attribute to (no meaningful RIM path).
    pub fn tenancy_path(&self) -> Option<Vec<String>> {
        let client_id = self.client_id()?;
        Some(vec![
            "root".to_string(),
            self.category().as_str().to_string(),
            client_id.to_string(),
        ])
    }

    pub fn tenancy_path_v2(&self) -> Option<Vec<String>> {
        let (level_3, level_4, level_5) = match self {
            Self::CiSandcastle {
                client_id,
                ci_purpose,
                sandcastle_job_id,
            } => (
                ci_purpose.as_deref()?,
                client_id.as_deref()?,
                sandcastle_job_id.as_deref()?,
            ),
            Self::DevEnv {
                on_demand_type,
                client_region,
                client_hostname,
                ..
            } => (
                on_demand_type.as_deref()?,
                client_region.as_deref()?,
                client_hostname.as_deref()?,
            ),
            Self::Mast {
                data_project,
                offline_job_root_run_id,
                offline_job_leaf_run_id,
                ..
            } => (
                data_project.as_deref()?,
                offline_job_root_run_id.as_deref()?,
                offline_job_leaf_run_id.as_deref()?,
            ),
            Self::FaaS {
                client_id,
                atlas_purpose,
                atlas_env_id,
                ..
            } => (
                atlas_purpose.as_deref()?,
                client_id.as_deref()?,
                atlas_env_id.as_deref()?,
            ),
            _ => {
                let client_id = self.client_id()?;
                (client_id, client_id, client_id)
            }
        };

        Some(vec![
            "root".to_string(),
            self.category().as_str().to_string(),
            level_3.to_string(),
            level_4.to_string(),
            level_5.to_string(),
        ])
    }
}

impl fmt::Display for TenantInfo {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(fmt, "{}", self.tenancy_path().unwrap_or_default().join("/"))
    }
}

#[cfg(test)]
mod tests {
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn test_ipv6_identity() {
        let id = MononokeIdentity::from_str("MACHINE:2621:10d:c1a8:12c9::1162").unwrap();
        assert_eq!(id.id_data(), "2621:10d:c1a8:12c9::1162");
    }

    fn tenant_info(category: ClientCategory, client_id: &str) -> TenantInfo {
        let client_id = Some(client_id.to_string());
        match category {
            ClientCategory::HealthCheck => TenantInfo::HealthCheck { client_id },
            ClientCategory::InteractiveDev => TenantInfo::InteractiveDev { client_id },
            ClientCategory::DevEnv => TenantInfo::DevEnv {
                client_id,
                on_demand_type: None,
                client_region: None,
                client_hostname: None,
            },
            ClientCategory::CiSandcastle => TenantInfo::CiSandcastle {
                client_id,
                ci_purpose: None,
                sandcastle_job_id: None,
            },
            ClientCategory::SandcastleAutomation => TenantInfo::SandcastleAutomation { client_id },
            ClientCategory::Mast => TenantInfo::Mast {
                client_id,
                data_project: None,
                offline_job_root_run_id: None,
                offline_job_leaf_run_id: None,
            },
            ClientCategory::FaaS => TenantInfo::FaaS {
                client_id,
                atlas_env_id: None,
                atlas_rl: None,
                atlas_purpose: None,
                faas_job_name: None,
            },
            ClientCategory::Automation => TenantInfo::Automation { client_id },
            ClientCategory::Unknown => TenantInfo::Unknown { client_id },
        }
    }

    #[mononoke::test]
    fn test_tenancy_path_v2() {
        let interactive = tenant_info(ClientCategory::InteractiveDev, "USER:alice");
        assert_eq!(
            interactive.tenancy_path(),
            Some(vec![
                "root".to_string(),
                "interactive_dev".to_string(),
                "USER:alice".to_string(),
            ])
        );
        assert_eq!(
            interactive.tenancy_path_v2(),
            Some(vec![
                "root".to_string(),
                "interactive_dev".to_string(),
                "USER:alice".to_string(),
                "USER:alice".to_string(),
                "USER:alice".to_string(),
            ])
        );
    }

    #[mononoke::test]
    fn test_ci_sandcastle_tenancy_path_v2() {
        let ci = TenantInfo::CiSandcastle {
            client_id: Some("ALIAS:continuous".to_string()),
            ci_purpose: Some("ci_fbsource".to_string()),
            sandcastle_job_id: Some("1234".to_string()),
        };
        assert_eq!(
            ci.tenancy_path_v2(),
            Some(vec![
                "root".to_string(),
                "ci_sandcastle".to_string(),
                "ci_fbsource".to_string(),
                "ALIAS:continuous".to_string(),
                "1234".to_string(),
            ])
        );

        assert_eq!(
            tenant_info(ClientCategory::CiSandcastle, "ALIAS:continuous").tenancy_path_v2(),
            None
        );
    }

    #[mononoke::test]
    fn test_dev_env_tenancy_path_v2() {
        let dev_env = TenantInfo::DevEnv {
            client_id: Some("SERVICE_IDENTITY:ondemand_worker".to_string()),
            on_demand_type: Some("www_fbsource_configerator".to_string()),
            client_region: Some("lla3".to_string()),
            client_hostname: Some("od1689.lla3.facebook.com".to_string()),
        };
        assert_eq!(
            dev_env.tenancy_path_v2(),
            Some(vec![
                "root".to_string(),
                "dev_env".to_string(),
                "www_fbsource_configerator".to_string(),
                "lla3".to_string(),
                "od1689.lla3.facebook.com".to_string(),
            ])
        );
        assert_eq!(
            tenant_info(ClientCategory::DevEnv, "SERVICE_IDENTITY:ondemand_worker")
                .tenancy_path_v2(),
            None
        );
    }

    #[mononoke::test]
    fn test_mast_tenancy_path_v2() {
        let mast = TenantInfo::Mast {
            client_id: Some("DATA_PROJECT:genai_llm_research-agents".to_string()),
            data_project: Some("DATA_PROJECT:genai_llm_research-agents".to_string()),
            offline_job_root_run_id: Some("OFFLINE_JOB_ROOT_RUN_ID:mast-job-run/root".to_string()),
            offline_job_leaf_run_id: Some(
                "OFFLINE_JOB_LEAF_RUN_ID:mast-job-run/root.0".to_string(),
            ),
        };
        assert_eq!(
            mast.tenancy_path_v2(),
            Some(vec![
                "root".to_string(),
                "mast".to_string(),
                "DATA_PROJECT:genai_llm_research-agents".to_string(),
                "OFFLINE_JOB_ROOT_RUN_ID:mast-job-run/root".to_string(),
                "OFFLINE_JOB_LEAF_RUN_ID:mast-job-run/root.0".to_string(),
            ])
        );
        assert_eq!(
            tenant_info(
                ClientCategory::Mast,
                "DATA_PROJECT:genai_llm_research-agents"
            )
            .tenancy_path_v2(),
            None
        );
    }

    #[mononoke::test]
    fn test_faas_tenancy_path_v2() {
        for workload in ["ASYNC_JOB_ID:1234", "CREWMATE:5678"] {
            let faas = TenantInfo::FaaS {
                client_id: Some(workload.to_string()),
                atlas_purpose: Some("general".to_string()),
                atlas_env_id: Some("atlas-1234".to_string()),
                atlas_rl: None,
                faas_job_name: None,
            };
            assert_eq!(
                faas.tenancy_path_v2(),
                Some(vec![
                    "root".to_string(),
                    "faas".to_string(),
                    "general".to_string(),
                    workload.to_string(),
                    "atlas-1234".to_string(),
                ])
            );
        }
        assert_eq!(
            tenant_info(ClientCategory::FaaS, "ASYNC_JOB_ID:1234").tenancy_path_v2(),
            None
        );
    }

    #[mononoke::test]
    fn test_tenancy_path_v2_repeats_client_id_for_all_categories() {
        for category in [
            ClientCategory::HealthCheck,
            ClientCategory::InteractiveDev,
            ClientCategory::SandcastleAutomation,
            ClientCategory::Automation,
            ClientCategory::Unknown,
        ] {
            let tenant = tenant_info(category, "CLIENT:id");
            assert_eq!(
                tenant.tenancy_path_v2(),
                Some(vec![
                    "root".to_string(),
                    category.as_str().to_string(),
                    "CLIENT:id".to_string(),
                    "CLIENT:id".to_string(),
                    "CLIENT:id".to_string(),
                ])
            );
        }
    }

    #[mononoke::test]
    fn test_to_typed_string_thin() {
        let id = MononokeIdentity::from_legacy_type_data("SERVICE", "some_service");
        // In fbcode the identity is rendered through the C++ canonical
        // logging formatter at `access/if/AuthenticatedIdentity.cpp`,
        // producing the `AuthenticatedIdentity{...}` debug shape with the
        // default source (UNKNOWN) and an empty attribute list. In OSS the
        // formatter is unavailable, so the fallback emits a plain
        // `TYPE:data` summary.
        #[cfg(fbcode_build)]
        assert_eq!(
            id.to_typed_string(),
            "AuthenticatedIdentity{identity=SERVICE:some_service, source=UNKNOWN, attributes=[]}",
        );
        #[cfg(not(fbcode_build))]
        assert_eq!(id.to_typed_string(), "SERVICE:some_service");
    }

    #[cfg(not(fbcode_build))]
    #[mononoke::test]
    fn test_to_typed_string_with_attributes() {
        let auth_id = AuthenticatedIdentity {
            identity: crate::oss::Identity {
                id_type: "USER".to_string(),
                id_data: "mzr".to_string(),
            },
            attributes: vec![crate::oss::Attribute {
                identifier: crate::oss::AttributeKey {
                    attributeName: "id".to_string(),
                    attributeNamespace: "agent".to_string(),
                },
                value: crate::oss::IdentityAttribute {
                    attributeValue: "AGENT:devmate".to_string(),
                },
                val: "AGENT:devmate".to_string(),
            }],
        };
        let id = MononokeIdentity::from(auth_id);
        // OSS build: C++ formatter unavailable, so the fallback emits a
        // plain `TYPE:data` summary and drops attributes (the OSS path has
        // no way to mirror the C++ debug form without the formatter).
        assert_eq!(id.to_typed_string(), "USER:mzr");
    }
}
