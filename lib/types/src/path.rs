// Copyright Facebook, Inc. 2019
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use failure::Fallible;
use std::borrow::{Borrow, ToOwned};
use std::convert::AsRef;
use std::fmt;
use std::mem;
use std::ops::Deref;

#[derive(Clone, Debug, Default, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct RepoPathBuf(String);

#[derive(Debug, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct RepoPath(str);

impl RepoPathBuf {
    pub fn new() -> RepoPathBuf {
        Default::default()
    }

    pub fn from_string(s: String) -> Self {
        RepoPathBuf(s)
    }

    pub fn push<P: AsRef<RepoPath>>(&mut self, path: P) {
        self.append(&path.as_ref().0);
    }

    fn append(&mut self, s: &str) {
        if !self.0.is_empty() {
            self.0.push('/');
        }
        self.0.push_str(s);
    }
}

impl Deref for RepoPathBuf {
    type Target = RepoPath;
    fn deref(&self) -> &Self::Target {
        unsafe { mem::transmute(&*self.0) }
    }
}

impl AsRef<RepoPath> for RepoPathBuf {
    fn as_ref(&self) -> &RepoPath {
        self
    }
}

impl Borrow<RepoPath> for RepoPathBuf {
    fn borrow(&self) -> &RepoPath {
        self
    }
}

impl fmt::Display for RepoPathBuf {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&*self.0, formatter)
    }
}

impl RepoPath {
    pub fn from_utf8(s: &[u8]) -> Fallible<&RepoPath> {
        let utf8_str = std::str::from_utf8(s)?;
        Ok(RepoPath::from_str(utf8_str))
    }

    pub fn from_str(s: &str) -> &RepoPath {
        unsafe { mem::transmute(s) }
    }
}

impl AsRef<RepoPath> for RepoPath {
    fn as_ref(&self) -> &RepoPath {
        self
    }
}

impl ToOwned for RepoPath {
    type Owned = RepoPathBuf;
    fn to_owned(&self) -> Self::Owned {
        RepoPathBuf(self.0.to_string())
    }
}

impl fmt::Display for RepoPath {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.0, formatter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repo_path_initialization_with_invalid_utf8() {
        assert!(RepoPath::from_utf8(&vec![0x80, 0x80]).is_err());
    }

    #[test]
    fn test_path_display() {
        assert_eq!(
            format!("{}", RepoPath::from_utf8(b"slice").unwrap()),
            "slice"
        );
        assert_eq!(format!("{}", RepoPath::from_str("slice")), "slice");
    }

    #[test]
    fn test_path_debug() {
        assert_eq!(
            format!("{:?}", RepoPath::from_utf8(b"slice").unwrap()),
            "RepoPath(\"slice\")"
        );
        assert_eq!(
            format!("{:?}", RepoPath::from_str("slice")),
            "RepoPath(\"slice\")"
        );
    }

    #[test]
    fn test_pathbuf_display() {
        assert_eq!(format!("{}", RepoPathBuf::new()), "");
        assert_eq!(
            format!("{}", RepoPathBuf::from_string(String::from("slice"))),
            "slice"
        );
    }

    #[test]
    fn test_pathbuf_debug() {
        assert_eq!(format!("{:?}", RepoPathBuf::new()), "RepoPathBuf(\"\")");
        assert_eq!(
            format!("{:?}", RepoPathBuf::from_string(String::from("slice"))),
            "RepoPathBuf(\"slice\")"
        );
    }

    #[test]
    fn test_repo_path_conversions() {
        let repo_path_buf = RepoPathBuf::from_string(String::from("path_buf"));
        assert_eq!(repo_path_buf.as_ref().to_owned(), repo_path_buf);

        let repo_path = RepoPath::from_str("path");
        assert_eq!(repo_path.to_owned().as_ref(), repo_path);
    }

    #[test]
    fn test_repo_path_push() {
        let mut repo_path_buf = RepoPathBuf::new();
        repo_path_buf.push(RepoPath::from_str("one"));
        assert_eq!(repo_path_buf.as_ref(), RepoPath::from_str("one"));
        repo_path_buf.push(RepoPath::from_str("two"));
        assert_eq!(repo_path_buf.as_ref(), RepoPath::from_str("one/two"));
    }
}
