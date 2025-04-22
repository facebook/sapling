/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Borrow;
use std::fmt;
use std::io;
use std::ops::Deref;

use serde::Deserialize;
use serde::Serialize;

use crate::invalid;

/// Valid reference name. Non-empty string. Without `\n` in it.
#[derive(
    Clone,
    Debug,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Hash,
    Serialize,
    Deserialize
)]
#[serde(try_from = "String")]
pub struct RefName(String);

impl TryFrom<String> for RefName {
    type Error = io::Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() || value.contains('\n') {
            return Err(invalid(format!("invalid reference name: {:?}", &value)));
        }
        Ok(Self(value))
    }
}

impl Deref for RefName {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a> TryFrom<&'a str> for RefName {
    type Error = io::Error;

    fn try_from(value: &'a str) -> Result<Self, Self::Error> {
        Self::try_from(value.to_string())
    }
}

impl fmt::Display for RefName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Borrow<str> for RefName {
    fn borrow(&self) -> &str {
        self.0.as_str()
    }
}

impl Borrow<String> for RefName {
    fn borrow(&self) -> &String {
        &self.0
    }
}
