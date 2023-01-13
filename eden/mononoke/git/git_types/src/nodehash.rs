/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! A hash of a node (changeset, manifest or file).

use std::fmt;
use std::fmt::Display;
use std::result;
use std::str::FromStr;

use abomonation_derive::Abomonation;
use anyhow::Result;
use mononoke_types::hash::GitSha1;
use mononoke_types::sha1_hash::Sha1;
use mononoke_types::sha1_hash::Sha1Prefix;

/// An identifier for a changeset hash prefix in Nercurial.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
#[derive(Abomonation)]
pub struct GitSha1Prefix(Sha1Prefix);

impl GitSha1Prefix {
    pub const fn new(sha1prefix: Sha1Prefix) -> Self {
        GitSha1Prefix(sha1prefix)
    }

    pub fn from_bytes<B: AsRef<[u8]> + ?Sized>(bytes: &B) -> Result<Self> {
        Sha1Prefix::from_bytes(bytes).map(Self::new)
    }

    #[inline]
    pub fn min_cs(&self) -> GitSha1 {
        GitSha1::from_bytes(self.min_as_ref()).expect("Min sha1 is a valid sha1")
    }

    #[inline]
    pub fn max_cs(&self) -> GitSha1 {
        GitSha1::from_bytes(self.max_as_ref()).expect("Max sha1 is a valid sha1")
    }

    #[inline]
    pub fn min_as_ref(&self) -> &[u8] {
        self.0.min_as_ref()
    }

    #[inline]
    pub fn max_as_ref(&self) -> &[u8] {
        self.0.max_as_ref()
    }

    #[inline]
    pub fn into_git_sha1(self) -> Option<GitSha1> {
        self.0
            .into_sha1()
            .map(Sha1::into_byte_array)
            .map(GitSha1::from_byte_array)
    }
}

impl FromStr for GitSha1Prefix {
    type Err = <Sha1Prefix as FromStr>::Err;
    fn from_str(s: &str) -> result::Result<GitSha1Prefix, Self::Err> {
        Sha1Prefix::from_str(s).map(GitSha1Prefix)
    }
}

impl Display for GitSha1Prefix {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(fmt)
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
/// The type for resolving changesets by prefix of the hash
pub enum GitSha1sResolvedFromPrefix {
    /// Found single changeset
    Single(GitSha1),
    /// Found several changesets within the limit provided
    Multiple(Vec<GitSha1>),
    /// Found too many changesets exceeding the limit provided
    TooMany(Vec<GitSha1>),
    /// Changeset was not found
    NoMatch,
}
