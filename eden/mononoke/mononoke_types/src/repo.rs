/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::str::FromStr;

use abomonation_derive::Abomonation;
use anyhow::Error;
use lazy_static::lazy_static;
use regex::Regex;
use serde_derive::Serialize;
use sql::mysql;

use crate::errors::ErrorKind;

pub const REPO_ID_PREFIX: &str = "repo";
pub const REPO_ID_SUFFIX: &str = ".";
pub const REPO_ID_SUFFIX_PATTERN: &str = r"\.";

pub const EPH_ID_PREFIX: &str = "eph";
pub const EPH_ID_SUFFIX: &str = ".";
pub const EPH_ID_SUFFIX_PATTERN: &str = r"\.";

lazy_static! {
    /// Matches the repo prefix for repo-specific keys.
    pub static ref REPO_PREFIX_REGEX: Regex = Regex::new(
        format!(r"^{}(\d{{3}}\d+){}", REPO_ID_PREFIX, REPO_ID_SUFFIX_PATTERN
    ).as_str()).unwrap();

    /// Matches the ephemeral and repo prefix for repo-specific ephemeral
    /// keys.
    pub static ref EPH_REPO_PREFIX_REGEX: Regex = Regex::new(
        format!(
            r"^{}(\d+){}{}(\d{{3}}\d+){}",
            EPH_ID_PREFIX,
            EPH_ID_SUFFIX_PATTERN,
            REPO_ID_PREFIX,
            REPO_ID_SUFFIX_PATTERN,
    ).as_str()).unwrap();
}

/// Represents a repository. This ID is used throughout storage.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash, Abomonation)]
#[derive(Serialize, mysql::OptTryFromRowField)]
pub struct RepositoryId(i32);

impl RepositoryId {
    // TODO: (rain1) T30368905 Instead of using this struct directly, have a wrapper around it that
    // only accepts a u32.
    #[inline]
    pub const fn new(id: i32) -> Self {
        Self(id)
    }

    #[inline]
    pub fn id(&self) -> i32 {
        self.0
    }

    /// Generate a prefix suitable for a blobstore.
    #[inline]
    pub fn prefix(&self) -> String {
        // Generate repo0001, repo0002, etc.
        format!("{}{:04}{}", REPO_ID_PREFIX, self.0, REPO_ID_SUFFIX)
    }
}

impl fmt::Display for RepositoryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Default for RepositoryId {
    fn default() -> Self {
        Self::new(0)
    }
}

impl FromStr for RepositoryId {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<u32>()
            .map_err(|_| ErrorKind::FailedToParseRepositoryId(s.to_owned()).into())
            .map(|repoid| Self::new(repoid as i32))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn prefix_generation() {
        assert_eq!(RepositoryId::new(0).prefix().as_str(), "repo0000.");
        assert_eq!(RepositoryId::new(1).prefix().as_str(), "repo0001.");
        assert_eq!(RepositoryId::new(99).prefix().as_str(), "repo0099.");
        assert_eq!(RepositoryId::new(456).prefix().as_str(), "repo0456.");
        assert_eq!(RepositoryId::new(9999).prefix().as_str(), "repo9999.");
        assert_eq!(RepositoryId::new(12000).prefix().as_str(), "repo12000.");
    }

    #[test]
    fn prefix_match() {
        // Check generated prefixes match the expected form
        assert!(REPO_PREFIX_REGEX.is_match(RepositoryId::new(0).prefix().as_str()));
        assert!(REPO_PREFIX_REGEX.is_match(RepositoryId::new(1).prefix().as_str()));
        assert!(REPO_PREFIX_REGEX.is_match(RepositoryId::new(99).prefix().as_str()));
        assert!(REPO_PREFIX_REGEX.is_match(RepositoryId::new(456).prefix().as_str()));
        assert!(REPO_PREFIX_REGEX.is_match(RepositoryId::new(9999).prefix().as_str()));
        assert!(REPO_PREFIX_REGEX.is_match(RepositoryId::new(12000).prefix().as_str()));
    }

    #[test]
    fn prefix_not_match() {
        // Check we dont match unexpected forms
        assert!(!REPO_PREFIX_REGEX.is_match(""));
        assert!(!REPO_PREFIX_REGEX.is_match("."));
        assert!(!REPO_PREFIX_REGEX.is_match(".repo0000."));
        assert!(!REPO_PREFIX_REGEX.is_match("repo0000"));
        assert!(!REPO_PREFIX_REGEX.is_match("repo(0000)"));
        assert!(!REPO_PREFIX_REGEX.is_match("repo0000a"));
        assert!(!REPO_PREFIX_REGEX.is_match("repo00a0."));
        assert!(!REPO_PREFIX_REGEX.is_match("flat/repo0000."));
        assert!(!REPO_PREFIX_REGEX.is_match("repo."));
        assert!(!REPO_PREFIX_REGEX.is_match("repo0."));
        assert!(!REPO_PREFIX_REGEX.is_match("repo00."));
        assert!(!REPO_PREFIX_REGEX.is_match("repo000."));
    }
}
