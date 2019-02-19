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

#[derive(Clone, Debug, Default, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct PathComponentBuf(String);

#[derive(Debug, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct PathComponent(str);

const SEPARATOR: char = '/';

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
            self.0.push(SEPARATOR);
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

    pub fn components(&self) -> impl Iterator<Item = &PathComponent> {
        self.0.split(SEPARATOR).map(|s| PathComponent::from_str(s))
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

impl PathComponentBuf {
    pub fn from_string(s: String) -> Self {
        PathComponentBuf(s)
    }
}

impl Deref for PathComponentBuf {
    type Target = PathComponent;
    fn deref(&self) -> &Self::Target {
        unsafe { mem::transmute(&*self.0) }
    }
}

impl AsRef<PathComponent> for PathComponentBuf {
    fn as_ref(&self) -> &PathComponent {
        self
    }
}

impl Borrow<PathComponent> for PathComponentBuf {
    fn borrow(&self) -> &PathComponent {
        self
    }
}

impl fmt::Display for PathComponentBuf {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&*self.0, formatter)
    }
}

impl PathComponent {
    pub fn from_utf8(s: &[u8]) -> Fallible<&PathComponent> {
        let utf8_str = std::str::from_utf8(s)?;
        Ok(PathComponent::from_str(utf8_str))
    }

    pub fn from_str(s: &str) -> &PathComponent {
        unsafe { mem::transmute(s) }
    }
}

impl AsRef<PathComponent> for PathComponent {
    fn as_ref(&self) -> &PathComponent {
        self
    }
}

impl AsRef<RepoPath> for PathComponent {
    fn as_ref(&self) -> &RepoPath {
        RepoPath::from_str(&self.0)
    }
}

impl ToOwned for PathComponent {
    type Owned = PathComponentBuf;
    fn to_owned(&self) -> Self::Owned {
        PathComponentBuf(self.0.to_string())
    }
}

impl fmt::Display for PathComponent {
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

    #[test]
    fn test_component_initialization_with_invalid_utf8() {
        assert!(PathComponent::from_utf8(&vec![0x80, 0x80]).is_err());
    }

    #[test]
    fn test_component_display() {
        assert_eq!(
            format!("{}", PathComponent::from_utf8(b"slice").unwrap()),
            "slice"
        );
        assert_eq!(format!("{}", PathComponent::from_str("slice")), "slice");
    }

    #[test]
    fn test_component_debug() {
        assert_eq!(
            format!("{:?}", PathComponent::from_utf8(b"slice").unwrap()),
            "PathComponent(\"slice\")"
        );
        assert_eq!(
            format!("{:?}", PathComponent::from_str("slice")),
            "PathComponent(\"slice\")"
        );
    }

    #[test]
    fn test_componentbuf_display() {
        assert_eq!(
            format!("{}", PathComponentBuf::from_string(String::from("slice"))),
            "slice"
        );
    }

    #[test]
    fn test_componentbuf_debug() {
        assert_eq!(
            format!("{:?}", PathComponentBuf::from_string(String::from("slice"))),
            "PathComponentBuf(\"slice\")"
        );
    }

    #[test]
    fn test_component_conversions() {
        let componentbuf = PathComponentBuf::from_string(String::from("componentbuf"));
        assert_eq!(componentbuf.as_ref().to_owned(), componentbuf);

        let component = PathComponent::from_str("component");
        assert_eq!(component.to_owned().as_ref(), component);
    }

    #[test]
    fn test_path_components() {
        let mut iter = RepoPath::from_str("foo/bar/baz.txt").components();
        assert_eq!(iter.next().unwrap(), PathComponent::from_str("foo"));
        assert_eq!(iter.next().unwrap(), PathComponent::from_str("bar"));
        assert_eq!(iter.next().unwrap(), PathComponent::from_str("baz.txt"));
        assert!(iter.next().is_none());
    }

    #[test]
    fn test_append_component_to_path() {
        let expected = RepoPathBuf::from_string(String::from("foo/bar/baz.txt"));
        let mut pathbuf = RepoPathBuf::new();
        for component in expected.components() {
            pathbuf.push(component);
        }
        assert_eq!(pathbuf, expected);
    }
}
