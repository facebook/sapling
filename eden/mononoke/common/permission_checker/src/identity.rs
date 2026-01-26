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
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;

#[cfg(not(fbcode_build))]
use crate::oss::AuthenticatedIdentity;

pub type MononokeIdentitySet = BTreeSet<MononokeIdentity>;

#[derive(Clone, Debug)]
pub enum MononokeIdentity {
    TypeData { id_type: String, id_data: String },
    Authenticated(AuthenticatedIdentity),
}

// Manual implementations for Eq, PartialEq, Hash, Ord, PartialOrd
// that compare based on id_type and id_data, regardless of variant
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
    pub fn new(id_type: impl Into<String>, id_data: impl Into<String>) -> Self {
        Self::TypeData {
            id_type: id_type.into(),
            id_data: id_data.into(),
        }
    }

    pub fn id_type(&self) -> &str {
        match self {
            Self::TypeData { id_type, .. } => id_type.as_str(),
            Self::Authenticated(auth_id) => auth_id.identity.id_type.as_str(),
        }
    }

    pub fn variant(&self) -> &str {
        match self {
            Self::TypeData { .. } => "TypeData",
            Self::Authenticated(_) => "Authenticated",
        }
    }

    pub fn id_data(&self) -> &str {
        match self {
            Self::TypeData { id_data, .. } => id_data.as_str(),
            Self::Authenticated(auth_id) => auth_id.identity.id_data.as_str(),
        }
    }

    pub fn is_of_type(&self, id_type: &str) -> bool {
        self.id_type() == id_type
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
            format!(
                "MononokeIdentity parse error, expected TYPE:data, got {:?}",
                value
            )
        })?;
        Ok(Self::new(ty, data))
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
    fn is_quicksand(&self) -> bool;

    fn likely_an_agent(&self) -> bool;

    fn is_proxygen_test_identity(&self) -> bool;

    fn hostprefix(&self) -> Option<&str>;

    fn hostname(&self) -> Option<&str>;

    fn username(&self) -> Option<&str>;
    fn identity_type_filtered_concat(&self, id_type: &str) -> Option<String>;
    fn main_client_identity(
        &self,
        sandcastle_alias: Option<&str>,
        clientinfo_atlas_env_id: Option<&str>,
    ) -> String;

    fn to_string(&self) -> String;
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
}
