/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt;
use std::str::FromStr;

use serde::Deserialize;
use serde::Serialize;

#[derive(
    Serialize,
    Deserialize,
    Debug,
    Clone,
    Copy,
    Eq,
    PartialEq,
    Ord,
    PartialOrd
)]
pub enum Phase {
    /// Public commits and their ancestors are immutable, already shared widely
    /// with others (ex. the server), usually defined by a remote bookmark (ex.
    /// remote/main).
    #[serde(rename = "public")]
    Public,
    /// Draft commits and their ancestors (except for public commits) are
    /// mutable, narrowly shared with others, and visible.
    #[serde(rename = "draft")]
    Draft,
}

impl fmt::Display for Phase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Public => "public",
            Self::Draft => "draft",
        };
        f.write_str(name)
    }
}

impl FromStr for Phase {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "public" => Ok(Self::Public),
            "draft" => Ok(Self::Draft),
            _ => Err(format!("unknown phase: {}", s)),
        }
    }
}
