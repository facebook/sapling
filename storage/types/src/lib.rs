// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Types shared across the Mononoke storage layer.

#![deny(warnings)]

extern crate rand;
extern crate serde;
#[macro_use]
extern crate serde_derive;

/// Versions are used to ensure consistency of state across all users of the bookmark store.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct Version(pub Option<u64>);

impl Version {
    pub fn absent() -> Self {
        Version::default()
    }
}

impl From<u64> for Version {
    fn from(v: u64) -> Self {
        Version(Some(v))
    }
}

impl Default for Version {
    fn default() -> Self {
        Version(None)
    }
}

pub fn version_random() -> Version {
    Version::from(rand::random::<u64>())
}
