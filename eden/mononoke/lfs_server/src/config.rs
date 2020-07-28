/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow;
use permission_checker::MononokeIdentity;
use serde::de::{Deserializer, Error};
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;
use std::default::Default;
use std::str::FromStr;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawLimit {
    pub counter: String,
    pub limit: i64,
    pub sleep_ms: i64,
    pub max_jitter_ms: i64,
    pub client_identities: Vec<String>,
}

/// Struct representing actual config data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawServerConfig {
    pub track_bytes_sent: bool,
    pub enable_consistent_routing: bool,
    pub disable_hostname_logging: bool,
    pub throttle_limits: Vec<RawLimit>,
    pub enforce_acl_check: bool,
    /// SCS counter category to use for blob popularity.
    pub object_popularity_category: Option<String>,
    /// Objects requested more than object_popularity_threshold recently (look at batch.rs for the
    /// time window) will not be consistently-routed. This ensures the full pool of servers can be
    /// used to serve very popular blobs.
    pub object_popularity_threshold: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct Limit {
    raw_limit: RawLimit,
    client_identities: Vec<MononokeIdentity>,
}

impl TryFrom<&RawLimit> for Limit {
    type Error = anyhow::Error;

    fn try_from(value: &RawLimit) -> Result<Self, Self::Error> {
        let client_identities = value
            .client_identities
            .iter()
            .map(|x| FromStr::from_str(&x))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            raw_limit: value.clone(),
            client_identities,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub raw_server_config: RawServerConfig,
    throttle_limits: Vec<Limit>,
}

impl<'de> Deserialize<'de> for ServerConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw_server_config = RawServerConfig::deserialize(deserializer)?;
        let try_throttle_limits = raw_server_config
            .throttle_limits
            .iter()
            .map(Limit::try_from)
            .collect::<Result<Vec<_>, _>>();

        let throttle_limits = match try_throttle_limits {
            Err(e) => return Err(D::Error::custom(e.to_string())),
            Ok(v) => v,
        };

        Ok(Self {
            raw_server_config,
            throttle_limits,
        })
    }
}

impl Serialize for ServerConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawServerConfig::serialize(&self.raw_server_config, serializer)
    }
}

impl Default for RawServerConfig {
    fn default() -> Self {
        Self {
            track_bytes_sent: false,
            enable_consistent_routing: false,
            disable_hostname_logging: false,
            throttle_limits: vec![],
            enforce_acl_check: false,
            object_popularity_category: None,
            object_popularity_threshold: None,
        }
    }
}
impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            raw_server_config: RawServerConfig::default(),
            throttle_limits: vec![],
        }
    }
}

impl ServerConfig {
    pub fn track_bytes_sent(&self) -> bool {
        self.raw_server_config.track_bytes_sent
    }
    pub fn enable_consistent_routing(&self) -> bool {
        self.raw_server_config.enable_consistent_routing
    }
    pub fn disable_hostname_logging(&self) -> bool {
        self.raw_server_config.disable_hostname_logging
    }
    pub fn throttle_limits(&self) -> Vec<Limit> {
        self.throttle_limits.clone()
    }
    pub fn enforce_acl_check(&self) -> bool {
        self.raw_server_config.enforce_acl_check
    }
    pub fn object_popularity_category(&self) -> Option<&str> {
        self.raw_server_config.object_popularity_category.as_deref()
    }
    pub fn object_popularity_threshold(&self) -> Option<u64> {
        self.raw_server_config.object_popularity_threshold
    }
}

impl Limit {
    pub fn counter(&self) -> String {
        self.raw_limit.counter.clone()
    }
    pub fn limit(&self) -> i64 {
        self.raw_limit.limit
    }
    pub fn sleep_ms(&self) -> i64 {
        self.raw_limit.sleep_ms
    }
    pub fn max_jitter_ms(&self) -> i64 {
        self.raw_limit.max_jitter_ms
    }
    pub fn client_identities(&self) -> Vec<MononokeIdentity> {
        self.client_identities.clone()
    }
}
