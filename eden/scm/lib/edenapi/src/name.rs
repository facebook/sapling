/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::str::FromStr;

use anyhow::Error;
use ascii::{AsAsciiStr, AsciiString};

use configparser::hg::FromConfigValue;

use crate::errors::EdenApiError;

/// A valididated repo name.
///
/// Presently, this is just a simple wrapper around a string that performs basic
/// sanitization by ensuring that the name consists of only ASCII characters.
///
/// In the future, the representation may change (e.g., it might become an enum
/// of all supported repos, or it might validate the name against a configured
/// list of valid repos).
#[derive(Clone, Debug)]
pub struct RepoName(AsciiString);

impl FromStr for RepoName {
    type Err = EdenApiError;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        let name = match name.as_ascii_str() {
            Ok(name) => name,
            _ => return Err(EdenApiError::InvalidRepoName(name.into())),
        };
        Ok(RepoName(name.to_ascii_string()))
    }
}

impl AsRef<str> for RepoName {
    fn as_ref(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for RepoName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl FromConfigValue for RepoName {
    fn try_from_str(s: &str) -> Result<Self, Error> {
        Ok(s.parse()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repo_name() {
        let valid: RepoName = "valid_name".parse().unwrap();

        assert_eq!(valid.as_ref(), "valid_name");
        assert_eq!(format!("{}", &valid), "valid_name");

        assert!(RepoName::from_str("invalid_name?foo=bar").is_ok());
        assert!(RepoName::from_str("\u{1F980}").is_err());
    }
}
