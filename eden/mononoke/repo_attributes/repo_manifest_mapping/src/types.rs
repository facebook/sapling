/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use sql::mysql;
use sql::mysql_async::FromValueError;
use sql::mysql_async::Value;
use sql::mysql_async::prelude::FromValue;

/// Stamp out a git-ref-name newtype whose SQL representation is raw bytes.
///
/// Modeled on `git_source_of_truth`'s `RepositoryName`: the value round-trips
/// as `Value::Bytes` so the underlying column is compared as binary (i.e.
/// case-sensitively), matching the production `_bin` collation intent.
macro_rules! byte_string_newtype {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Clone,
            Debug,
            Eq,
            Hash,
            Ord,
            PartialEq,
            PartialOrd,
            mysql::OptTryFromRowField
        )]
        pub struct $name(pub String);

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl From<$name> for Value {
            fn from(value: $name) -> Self {
                Value::Bytes(value.0.into_bytes())
            }
        }

        impl FromValue for $name {
            type Intermediate = $name;
        }

        impl TryFrom<Value> for $name {
            type Error = FromValueError;
            fn try_from(v: Value) -> Result<Self, FromValueError> {
                match v {
                    Value::Bytes(bytes) => match String::from_utf8(bytes) {
                        Ok(s) => Ok($name(s)),
                        Err(from_utf8_error) => {
                            Err(FromValueError(Value::Bytes(from_utf8_error.into_bytes())))
                        }
                    },
                    v => Err(FromValueError(v)),
                }
            }
        }
    };
}

byte_string_newtype!(
    /// The name of a manifest branch (a git ref name) within a manifest repo.
    ManifestBranch
);
byte_string_newtype!(
    /// The name of a member repo listed in a manifest (a git ref name).
    RepoName
);
byte_string_newtype!(
    /// The branch of a member repo listed in a manifest (a git ref name).
    RepoBranch
);

/// A single membership edge: the member repo `(repo_name, repo_branch)` belongs
/// to a manifest branch. The owning `(manifest_repo_id, manifest_branch)` is the
/// context in which the edge lives (it is supplied alongside a batch of edges),
/// so it is intentionally not part of the edge itself.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MembershipEdge {
    pub repo_name: RepoName,
    pub repo_branch: RepoBranch,
}

impl MembershipEdge {
    pub fn new(repo_name: RepoName, repo_branch: RepoBranch) -> Self {
        Self {
            repo_name,
            repo_branch,
        }
    }
}
