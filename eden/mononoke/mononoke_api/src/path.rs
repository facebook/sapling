/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;

use mononoke_types::MPath;
use mononoke_types::MPathElement;

use crate::errors::MononokeError;

// Define a wrapper around `Option<MPath>` to make it more convenient to
// use in the API.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MononokePath(Option<MPath>);

/// Whether this path is a path prefix of the given path.
/// `foo` is a prefix of `foo/bar`, but not of `foo1`.
pub fn is_prefix_of(lhs: Option<&MPath>, rhs: Option<&MPath>) -> bool {
    match (lhs, rhs) {
        (None, _) => true,
        (_, None) => false,
        (Some(lhs_mpath), Some(rhs_mpath)) => lhs_mpath.is_prefix_of(rhs_mpath),
    }
}

pub fn is_related_to(lhs: Option<&MPath>, rhs: Option<&MPath>) -> bool {
    is_prefix_of(lhs, rhs) || is_prefix_of(rhs, lhs)
}

impl MononokePath {
    pub fn new(path: Option<MPath>) -> Self {
        Self(path)
    }

    pub fn as_mpath(&self) -> Option<&MPath> {
        self.0.as_ref()
    }

    pub fn into_mpath(self) -> Option<MPath> {
        self.0
    }

    pub fn prefixes(&self) -> MononokePathPrefixes {
        MononokePathPrefixes::new(self)
    }

    /// Whether self is prefix of other or the other way arround.
    pub fn is_related_to(&self, other: &Self) -> bool {
        is_related_to(self.as_mpath(), other.as_mpath())
    }

    pub fn append(&self, element: &MPathElement) -> Self {
        Self(Some(MPath::join_opt_element(self.0.as_ref(), element)))
    }
}

// Because of conflicting generic traits, we cannot implement this
// generically for `AsRef<str>`.  Instead, implement the most common
// variants.
impl TryFrom<&str> for MononokePath {
    type Error = MononokeError;

    fn try_from(path: &str) -> Result<MononokePath, MononokeError> {
        if path.is_empty() {
            Ok(MononokePath(None))
        } else {
            let mpath = MPath::try_from(path)
                .map_err(|error| MononokeError::InvalidRequest(error.to_string()))?;
            Ok(MononokePath(Some(mpath)))
        }
    }
}

impl TryFrom<&String> for MononokePath {
    type Error = MononokeError;

    fn try_from(path: &String) -> Result<MononokePath, MononokeError> {
        MononokePath::try_from(path.as_str())
    }
}

impl From<MononokePath> for Option<MPath> {
    fn from(path: MononokePath) -> Option<MPath> {
        path.0
    }
}

impl From<MPath> for MononokePath {
    fn from(mpath: MPath) -> MononokePath {
        MononokePath(Some(mpath))
    }
}

impl From<Option<MPath>> for MononokePath {
    fn from(mpath: Option<MPath>) -> MononokePath {
        MononokePath(mpath)
    }
}

impl fmt::Display for MononokePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(ref mpath) = self.0 {
            return write!(f, "{}", mpath);
        }
        write!(f, "")
    }
}

impl fmt::Debug for MononokePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

/// An iterator over all the prefixes of a path.
pub struct MononokePathPrefixes {
    next_path: Option<MononokePath>,
}

impl MononokePathPrefixes {
    fn new(path: &MononokePath) -> Self {
        let next_path = path
            .as_mpath()
            .map(|mpath| MononokePath::new(mpath.split_dirname().0));
        MononokePathPrefixes { next_path }
    }
}

impl Iterator for MononokePathPrefixes {
    type Item = MononokePath;

    fn next(&mut self) -> Option<MononokePath> {
        match self.next_path.take() {
            None => None,
            Some(path) => {
                self.next_path = path
                    .as_mpath()
                    .map(|mpath| MononokePath::new(mpath.split_dirname().0));
                Some(path)
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn path_relations() -> Result<(), MononokeError> {
        let x = MononokePath::try_from("a/b/c")?;
        let y = MononokePath::try_from("a/b")?;
        let z = MononokePath::try_from("a/d")?;
        assert!(is_prefix_of(y.as_mpath(), x.as_mpath()));
        assert!(!is_prefix_of(z.as_mpath(), x.as_mpath()));
        assert!(is_prefix_of(x.as_mpath(), x.as_mpath()));
        assert!(!is_prefix_of(x.as_mpath(), y.as_mpath()));
        assert!(is_related_to(x.as_mpath(), y.as_mpath()));
        assert!(!is_related_to(x.as_mpath(), z.as_mpath()));
        Ok(())
    }
}
