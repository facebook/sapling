/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::convert::TryFrom;
use std::fmt;

use mononoke_types::MPath;

use crate::errors::MononokeError;

// Define a wrapper around `Option<MPath>` to make it more convenient to
// use in the API.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct MononokePath(Option<MPath>);

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
            Ok(MononokePath(Some(MPath::try_from(path)?)))
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
