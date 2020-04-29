/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{bail, Error, Result};
use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

pub type MononokeIdentitySet = BTreeSet<MononokeIdentity>;

#[derive(Clone, Eq, PartialEq, Debug, Hash, Ord, PartialOrd)]
pub struct MononokeIdentity {
    id_type: String,
    id_data: String,
}

impl MononokeIdentity {
    pub fn new(id_type: impl Into<String>, id_data: impl Into<String>) -> Result<Self> {
        let id_type = id_type.into();
        let id_data = id_data.into();

        #[cfg(fbcode_build)]
        {
            crate::facebook::verify_identity_type(&id_type)?;
        }

        Ok(Self { id_type, id_data })
    }

    pub fn id_type(&self) -> &str {
        &self.id_type
    }

    pub fn id_data(&self) -> &str {
        &self.id_data
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
        let mut parts = value.split(':');

        match (parts.next(), parts.next(), parts.next()) {
            (Some(ty), Some(data), None) => Self::new(ty, data),
            _ => bail!(
                "MononokeIdentity parse error, expected TYPE:data, got {:?}",
                value
            ),
        }
    }
}

#[cfg(not(fbcode_build))]
mod r#impl {
    use super::*;

    impl MononokeIdentity {
        pub fn reviewer_identities(_username: &str) -> MononokeIdentitySet {
            MononokeIdentitySet::new()
        }
    }
}
