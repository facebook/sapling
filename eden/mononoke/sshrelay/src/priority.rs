/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Error};
use std::fmt;
use std::str::FromStr;

const KEY: &str = "priority";

#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Default,
    Wishlist,
}

impl Default for Priority {
    fn default() -> Self {
        Self::Default
    }
}

impl FromStr for Priority {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "wishlist" => Ok(Self::Wishlist),
            _ => Err(format_err!("Invalid priority: {}", s)),
        }
    }
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Default => write!(f, "Default"),
            Self::Wishlist => write!(f, "Wishlist"),
        }
    }
}
