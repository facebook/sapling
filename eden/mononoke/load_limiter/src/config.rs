/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::{Error, Result};
use limits::types::{MononokeThrottleLimits, StaticSlicedLimit, StaticSlicedLimits};
use permission_checker::MononokeIdentitySet;
use serde::de::{Deserializer, Error as _};
use serde::Deserialize;
pub use session_id::SessionId;
use std::collections::BTreeSet;
use std::convert::{TryFrom, TryInto};
use std::str::FromStr;

#[derive(Clone)]
pub struct StaticSlicedLimitConfig {
    pub identities: MononokeIdentitySet,
    pub limit_pct: u64,
}

#[derive(Clone)]
pub struct StaticSlicedLimitsConfig {
    pub nonce: String,
    pub limits: Vec<StaticSlicedLimitConfig>,
}

#[derive(Clone)]
pub struct MononokeThrottleLimitsConfig {
    pub raw_config: MononokeThrottleLimits,
    pub static_sliced_limits: Option<StaticSlicedLimitsConfig>,
}

impl TryFrom<StaticSlicedLimit> for StaticSlicedLimitConfig {
    type Error = Error;

    fn try_from(inner: StaticSlicedLimit) -> Result<Self, Error> {
        let identity_set = inner
            .identities
            .iter()
            .map(|x| FromStr::from_str(&x))
            .collect::<Result<BTreeSet<_>, _>>()?;

        Ok(Self {
            limit_pct: inner.limit_pct.try_into()?,
            identities: identity_set,
        })
    }
}

impl TryFrom<StaticSlicedLimits> for StaticSlicedLimitsConfig {
    type Error = Error;

    fn try_from(inner: StaticSlicedLimits) -> Result<Self, Error> {
        Ok(Self {
            nonce: inner.nonce,
            limits: inner
                .limits
                .iter()
                .cloned()
                .map(StaticSlicedLimitConfig::try_from)
                .collect::<Result<Vec<_>, _>>()?,
        })
    }
}

impl TryFrom<MononokeThrottleLimits> for MononokeThrottleLimitsConfig {
    type Error = Error;

    fn try_from(inner: MononokeThrottleLimits) -> Result<Self, Error> {
        let ssl = inner.static_sliced_limits.clone();

        let static_sliced_limits = if let Some(ssl) = ssl {
            Some(ssl.try_into()?)
        } else {
            None
        };

        Ok(Self {
            raw_config: inner,
            static_sliced_limits,
        })
    }
}

impl<'de> Deserialize<'de> for MononokeThrottleLimitsConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = MononokeThrottleLimits::deserialize(deserializer)?;
        let config = Self::try_from(raw).map_err(|e| D::Error::custom(format!("{:?}", e)))?;
        Ok(config)
    }
}
