/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use std::collections::HashSet;

pub type MononokeIdentitySet = HashSet<MononokeIdentity>;

#[derive(Clone, Eq, PartialEq, Debug, Hash)]
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

#[cfg(not(fbcode_build))]
mod r#impl {
    use super::*;

    impl MononokeIdentity {
        pub fn reviewer_identities(_username: &str) -> MononokeIdentitySet {
            MononokeIdentitySet::new()
        }
    }
}
