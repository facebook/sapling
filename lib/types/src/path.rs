/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Here we have types for working with paths specialized for source control internals.
//! They are akin to str and String in high level behavior. `RepoPath` is an unsized type wrapping
//! a str so it can't be instantiated directly. `RepoPathBuf` represents the owned version of a
//! RepoPath and wraps a String.
//!
//! The inspiration for `RepoPath` and `RepoPathBuf` comes from the std::path crate however
//! we know that the internal representation of a path is consistently a utf8 string where
//! directories are delimited by the `SEPARATOR` (`/`) so our types can have a simpler
//! representation. It is because of the same reason that we can't use the abstractions in
//! `std::path` for internal uses where we need to apply the same algorithm for blobs we get from
//! the server across all systems.
//!
//! We could use `String` and `&str` directly however these types are inexpressive and have few
//! guarantees. Rust has a strong type system so we can leverage it to provide more safety.
//!
//! `PathComponent` and `PathComponentBuf` can be seen as specializations of `RepoPath` and
//! `RepoPathBuf` that do not have any `SEPARATOR` characters. The main operation that is done on
//! paths is iterating over its components. `PathComponents` are names of files or directories.
//! For the path: `foo/bar/baz.txt` we have 3 components: `foo`, `bar` and `baz.txt`.
//!
//! A lot of algorithms used in source control management operate on directories so having an
//! abstraction for individual components is going to increase readability in the long run.
//! A clear example for where we may want to use `PathComponentBuf` is in the treemanifest logic
//! where all indexing is done using components. The index in those cases must be able to own
//! component. Writing it in terms of `RepoPathBuf` would probably be less readable that
//! writing it in terms of `String`.

use std::{
    borrow::{Borrow, ToOwned},
    cmp::Ordering,
    convert::AsRef,
    fmt, mem,
    ops::Deref,
    str::Utf8Error,
};

use serde_derive::{Deserialize, Serialize};
use thiserror::Error;

#[cfg(any(test, feature = "for-tests"))]
use rand::Rng;

/// An owned version of a `RepoPath`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Hash)]
#[derive(Serialize, Deserialize)]
pub struct RepoPathBuf(String);

/// A normalized path starting from the root of the repository. Paths can be broken into
/// components by using `SEPARATOR`. Normalized means that it following the following rules:
///  * unicode is normalized
///  * does not end with a `SEPARATOR`
///  * does not contain:
///    * \0, null character - it is an illegal file name character on unix
///    * \1, CTRL-A - used as metadata separator
///    * \10, newline - used as metadata separator
///  * does not contain the following components:
///    * ``, empty, implies that paths can't start with, end or contain consecutive `SEPARATOR`s
///    * `.`, dot/period, unix current directory
///    * `..`, double dot, unix parent directory
/// TODO: There is more validation that could be done here. Windows has a broad list of illegal
/// characters and reseved words.
///
/// It should be noted that `RepoPathBuf` and `RepoPath` implement `AsRef<RepoPath>`.
#[derive(Debug, Eq, PartialEq, Hash, Serialize)]
pub struct RepoPath(str);

/// An owned version of a `PathComponent`. Not intended for mutation. RepoPathBuf is probably
/// more appropriate for mutation.
#[derive(Clone, Debug, Default, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct PathComponentBuf(String);

/// A `RepoPath` is a series of `PathComponent`s joined together by a separator (`/`).
/// Names for directories or files.
#[derive(Debug, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct PathComponent(str);

/// The One. The One Character We Use To Separate Paths Into Components.
pub const SEPARATOR: char = '/';

#[derive(Error, Debug)]
pub enum ParseError {
    ValidationError(String, ValidationError),
    InvalidUtf8(Vec<u8>, Utf8Error),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        match self {
            ParseError::ValidationError(path, validation_error) => {
                write!(f, "Failed to validate {:?}. {}", path, validation_error)
            }
            ParseError::InvalidUtf8(bytes, utf8_error) => write!(
                f,
                "Failed to parse to Utf8: {:?}. {}",
                String::from_utf8_lossy(bytes),
                utf8_error
            ),
        }
    }
}

#[derive(Error, Debug)]
pub enum ValidationError {
    #[error("Invalid component: \"{0}\".")]
    InvalidPathComponent(#[from] InvalidPathComponent),
    #[error("Invalid byte: {0:?}.")]
    InvalidByte(u8),
    #[error("Trailing slash.")]
    TrailingSlash,
}

#[derive(Error, Debug)]
pub enum InvalidPathComponent {
    #[error("")]
    Empty,
    #[error(".")]
    Current,
    #[error("..")]
    Parent,
}

impl RepoPathBuf {
    /// Constructs an empty RepoPathBuf. This path will have no
    /// components and will be equivalent to the root of the repository.
    pub fn new() -> RepoPathBuf {
        Default::default()
    }

    /// Constructs a `RepoPathBuf` from a vector of bytes. It will fail when the bytes are are not
    /// valid utf8 or when the string does not respect the `RepoPathBuf` rules.
    pub fn from_utf8(vec: Vec<u8>) -> Result<Self, ParseError> {
        let utf8_string = String::from_utf8(vec).map_err(|e| {
            let utf8_error = e.utf8_error();
            ParseError::InvalidUtf8(e.into_bytes(), utf8_error)
        })?;
        RepoPathBuf::from_string(utf8_string)
    }

    /// Constructs a `RepoPathBuf` from a `String`. It can fail when the contents of String is
    /// deemed invalid. See `RepoPath` for validation rules.
    pub fn from_string(s: String) -> Result<Self, ParseError> {
        match validate_path(&s) {
            Ok(()) => Ok(RepoPathBuf(s)),
            Err(e) => Err(ParseError::ValidationError(s, e)),
        }
    }

    /// Consumes the current instance and returns a String with the contents of this `RepoPathBuf`.
    /// Intended for code that converts between different formats. FFI / serialization.
    pub fn into_string(self) -> String {
        self.0
    }

    /// Converts the `RepoPathBuf` in a `RepoPath`.
    pub fn as_repo_path(&self) -> &RepoPath {
        self
    }

    /// Returns whether the current `RepoPathBuf` has no components. Since `RepoPathBuf`
    /// represents the relative path from the start of the repository this is equivalent to
    /// checking whether the path is the root of the repository
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Append a `RepoPath` to the end of `RepoPathBuf`. This function will add the `SEPARATOR`
    /// required by concatenation.
    pub fn push<P: AsRef<RepoPath>>(&mut self, path: P) {
        self.append(&path.as_ref().0);
    }

    /// Removed the last component from the `RepoPathBuf` and return it.
    pub fn pop(&mut self) -> Option<PathComponentBuf> {
        if self.0.is_empty() {
            return None;
        }
        match self.0.rfind(SEPARATOR) {
            None => {
                let result = PathComponentBuf::from_string_unchecked(self.0.clone());
                self.0 = String::new();
                Some(result)
            }
            Some(pos) => {
                let result = PathComponentBuf::from_string_unchecked(self.0.split_off(pos + 1));
                self.0.pop(); // remove SEPARATOR
                Some(result)
            }
        }
    }

    fn append(&mut self, s: &str) {
        if !self.0.is_empty() {
            self.0.push(SEPARATOR);
        }
        self.0.push_str(s);
    }
}

impl Ord for RepoPathBuf {
    fn cmp(&self, other: &RepoPathBuf) -> Ordering {
        self.as_repo_path().cmp(other.as_repo_path())
    }
}

impl PartialOrd for RepoPathBuf {
    fn partial_cmp(&self, other: &RepoPathBuf) -> Option<Ordering> {
        self.as_repo_path().partial_cmp(other.as_repo_path())
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

impl AsRef<[u8]> for RepoPathBuf {
    fn as_ref(&self) -> &[u8] {
        self.as_byte_slice()
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
    /// Returns an empty `RepoPath`. Parallel to `RepoPathBuf::new()`. This path will have no
    /// components and will be equivalent to the root of the repository.
    pub fn empty() -> &'static RepoPath {
        RepoPath::from_str_unchecked("")
    }

    /// Returns whether the current `RepoPath` has no components. Since `RepoPath`
    /// represents the relative path from the start of the repository this is equivalent to
    /// checking whether the path is the root of the repository
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Constructs a `RepoPath` from a byte slice. It will fail when the bytes are are not valid
    /// utf8 or when the string does not respect the `RepoPath` rules.
    pub fn from_utf8<'a, S: AsRef<[u8]> + ?Sized>(s: &'a S) -> Result<&'a RepoPath, ParseError> {
        let utf8_str = std::str::from_utf8(s.as_ref())
            .map_err(|e| ParseError::InvalidUtf8(s.as_ref().to_vec(), e))?;
        RepoPath::from_str(utf8_str)
    }

    /// Constructs a `RepoPath` from a `str` slice. It will fail when the string does not respect
    /// the `RepoPath` rules.
    pub fn from_str(s: &str) -> Result<&RepoPath, ParseError> {
        validate_path(s).map_err(|e| ParseError::ValidationError(s.to_string(), e))?;
        Ok(RepoPath::from_str_unchecked(s))
    }

    fn from_str_unchecked(s: &str) -> &RepoPath {
        unsafe { mem::transmute(s) }
    }

    /// Returns the underlying bytes of the `RepoPath`.
    pub fn as_byte_slice(&self) -> &[u8] {
        self.0.as_bytes()
    }

    /// Returns the `str` interpretation of the `RepoPath`.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Return the parent of the path. The empty path, `RepoPath::empty()` does not have a
    /// parent so `None` is returned in that case.
    pub fn parent(&self) -> Option<&RepoPath> {
        self.split_last_component().map(|(parent, _)| parent)
    }

    /// Return the last component of the path. The empty path, `RepoPath::empty()` does not have
    /// any components so `None` is returned in that case.
    pub fn last_component(&self) -> Option<&PathComponent> {
        self.split_last_component().map(|(_, component)| component)
    }

    /// Tries to split the current `RepoPath` in a parent path and a component. If the current
    /// path is empty then None is returned. If the current path contains only one component then
    /// the pair that is returned is the empty repo path and a path component that will match the
    /// contents `self`.
    pub fn split_last_component(&self) -> Option<(&RepoPath, &PathComponent)> {
        if self.is_empty() {
            return None;
        }
        match self.0.rfind(SEPARATOR) {
            Some(pos) => Some((
                RepoPath::from_str_unchecked(&self.0[..pos]),
                PathComponent::from_str_unchecked(&self.0[(pos + 1)..]),
            )),
            None => Some((
                RepoPath::empty(),
                PathComponent::from_str_unchecked(&self.0),
            )),
        }
    }

    /// Returns an iterator over the parents of the current path.
    /// The `RepoPath` itself is not returned. The root of the repository represented by the empty
    /// `RepoPath` is always returned by this iterator except if the path is empty.
    ///
    /// For example for the path `"foo/bar/baz"` this iterator will return three items:
    /// `""`, `"foo"` and `"foo/bar"`.
    ///
    /// If you don't want to handle the empty path, then you can use `parents().skip(1)`.
    /// It is possible to get iterate over parents with elements in paralel using:
    /// `path.parents().zip(path.components())`.
    pub fn parents<'a>(&'a self) -> Parents<'a> {
        Parents::new(self)
    }

    /// Returns an iterator over the components of the path.
    pub fn components<'a>(&'a self) -> Components<'a> {
        Components::new(self)
    }
}

impl Ord for RepoPath {
    fn cmp(&self, other: &RepoPath) -> Ordering {
        self.components().cmp(other.components())
    }
}

impl PartialOrd for RepoPath {
    fn partial_cmp(&self, other: &RepoPath) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl AsRef<RepoPath> for RepoPath {
    fn as_ref(&self) -> &RepoPath {
        self
    }
}

impl AsRef<[u8]> for RepoPath {
    fn as_ref(&self) -> &[u8] {
        self.as_byte_slice()
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
    /// Constructs an from a `String`. It can fail when the contents of `String` is deemed invalid.
    /// See `PathComponent` for validation rules.
    pub fn from_string(s: String) -> Result<Self, ParseError> {
        match validate_component(&s) {
            Ok(()) => Ok(PathComponentBuf(s)),
            Err(e) => Err(ParseError::ValidationError(s, e)),
        }
    }

    /// Consumes the current instance and returns a String with the contents of this
    /// `PathComponentBuf`.
    /// Intended for code that converts between different formats. FFI / serialization.
    pub fn into_string(self) -> String {
        self.0
    }

    /// Converts the `PathComponentBuf` in a `PathComponent`.
    pub fn as_path_component(&self) -> &PathComponent {
        self
    }

    fn from_string_unchecked(s: String) -> Self {
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
    /// Constructs a `PathComponent` from a byte slice. It will fail when the bytes are are not
    /// valid utf8 or when the string does not respect the `PathComponent` rules.
    pub fn from_utf8(s: &[u8]) -> Result<&PathComponent, ParseError> {
        let utf8_str =
            std::str::from_utf8(s).map_err(|e| ParseError::InvalidUtf8(s.to_vec(), e))?;
        PathComponent::from_str(utf8_str)
    }

    /// Constructs a `PathComponent` from a `str` slice. It will fail when the string does not
    /// respect the `PathComponent` rules.
    pub fn from_str(s: &str) -> Result<&PathComponent, ParseError> {
        validate_component(s).map_err(|e| ParseError::ValidationError(s.to_string(), e))?;
        Ok(PathComponent::from_str_unchecked(s))
    }

    fn from_str_unchecked(s: &str) -> &PathComponent {
        unsafe { mem::transmute(s) }
    }

    /// Returns the underlying bytes of the `PathComponent`.
    pub fn as_byte_slice(&self) -> &[u8] {
        self.0.as_bytes()
    }

    /// Returns the `str` interpretation of the `RepoPath`.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<PathComponent> for PathComponent {
    fn as_ref(&self) -> &PathComponent {
        self
    }
}

impl AsRef<RepoPath> for PathComponent {
    fn as_ref(&self) -> &RepoPath {
        unsafe { mem::transmute(&self.0) }
    }
}

impl AsRef<[u8]> for PathComponent {
    fn as_ref(&self) -> &[u8] {
        self.as_byte_slice()
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

fn validate_path(s: &str) -> Result<(), ValidationError> {
    if s.is_empty() {
        return Ok(());
    }
    if s.bytes().next_back() == Some(b'/') {
        return Err(ValidationError::TrailingSlash);
    }
    for component in s.split(SEPARATOR) {
        validate_component(component)?;
    }
    Ok(())
}

fn validate_component(s: &str) -> Result<(), ValidationError> {
    if s.is_empty() {
        return Err(InvalidPathComponent::Empty.into());
    }
    if s == "." {
        return Err(InvalidPathComponent::Current.into());
    }
    if s == ".." {
        return Err(InvalidPathComponent::Parent.into());
    }
    for b in s.bytes() {
        if b == 0u8 || b == 1u8 || b == b'\n' || b == b'/' {
            return Err(ValidationError::InvalidByte(b));
        }
    }
    Ok(())
}

pub struct Parents<'a> {
    path: &'a RepoPath,
    position: Option<usize>,
}

impl<'a> Parents<'a> {
    pub fn new(path: &'a RepoPath) -> Self {
        Parents {
            path,
            position: None,
        }
    }
}

impl<'a> Iterator for Parents<'a> {
    type Item = &'a RepoPath;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(ref mut position) = self.position {
            match self.path.0[*position..].find(SEPARATOR) {
                Some(delta) => {
                    let end = *position + delta;
                    let result = RepoPath::from_str_unchecked(&self.path.0[..end]);
                    *position = end + 1;
                    Some(result)
                }
                None => {
                    *position = self.path.0.len();
                    None
                }
            }
        } else {
            self.position = Some(0);
            if self.path.is_empty() {
                None
            } else {
                Some(RepoPath::empty())
            }
        }
    }
}

pub struct Components<'a> {
    path: &'a RepoPath,
    position: usize,
}

impl<'a> Components<'a> {
    pub fn new(path: &'a RepoPath) -> Self {
        Components { path, position: 0 }
    }
}

impl<'a> Iterator for Components<'a> {
    type Item = &'a PathComponent;

    fn next(&mut self) -> Option<Self::Item> {
        match self.path.0[self.position..].find(SEPARATOR) {
            Some(delta) => {
                let end = self.position + delta;
                let result = PathComponent::from_str_unchecked(&self.path.0[self.position..end]);
                self.position = end + 1;
                Some(result)
            }
            None => {
                if self.position < self.path.0.len() {
                    let result = PathComponent::from_str_unchecked(&self.path.0[self.position..]);
                    self.position = self.path.0.len();
                    Some(result)
                } else {
                    None
                }
            }
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl quickcheck::Arbitrary for RepoPathBuf {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        let size = g.gen_range(0, 8);
        let mut path_buf = RepoPathBuf::new();
        for _ in 0..size {
            path_buf.push(PathComponentBuf::arbitrary(g).as_ref());
        }
        path_buf
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl quickcheck::Arbitrary for PathComponentBuf {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        // Most strings should be valid `PathComponent` so it is reasonable to loop until a valid
        // string is found. To note that generating Arbitrary Unicode on `char` is implemented
        // using a loop where random bytes are validated against the `char` constructor.
        loop {
            let size = g.gen_range(1, 8);
            let mut s = String::with_capacity(size);
            for _ in 0..size {
                let c = loop {
                    let x = char::arbitrary(g);
                    if x != SEPARATOR {
                        break x;
                    }
                };
                s.push(c);
            }
            if let Ok(component) = PathComponentBuf::from_string(s) {
                return component;
            }
        }
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = PathComponentBuf>> {
        Box::new(
            self.0
                .shrink()
                .filter_map(|s| PathComponentBuf::from_string(s).ok()),
        )
    }
}

#[cfg(any(test, feature = "for-tests"))]
pub mod mocks {
    use super::*;
    use lazy_static::lazy_static;

    lazy_static! {
        pub static ref FOO_PATH: RepoPathBuf =
            RepoPathBuf::from_string(String::from("foo")).unwrap();
        pub static ref BAR_PATH: RepoPathBuf =
            RepoPathBuf::from_string(String::from("bar")).unwrap();
        pub static ref BAZ_PATH: RepoPathBuf =
            RepoPathBuf::from_string(String::from("baz")).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use quickcheck::quickcheck;

    use crate::testutil::{path_component, path_component_buf, repo_path, repo_path_buf};

    #[test]
    fn test_repo_path_initialization_with_invalid_utf8() {
        assert!(RepoPath::from_utf8(&vec![0x80, 0x80]).is_err());
        assert!(RepoPathBuf::from_utf8(vec![0x80, 0x80]).is_err());
    }

    #[test]
    fn test_path_display() {
        assert_eq!(
            format!("{}", RepoPath::from_utf8(b"slice").unwrap()),
            "slice"
        );
        assert_eq!(format!("{}", RepoPath::from_str("slice").unwrap()), "slice");
    }

    #[test]
    fn test_path_debug() {
        assert_eq!(
            format!("{:?}", RepoPath::from_utf8(b"slice").unwrap()),
            "RepoPath(\"slice\")"
        );
        assert_eq!(
            format!("{:?}", RepoPath::from_str("slice").unwrap()),
            "RepoPath(\"slice\")"
        );
    }

    #[test]
    fn test_pathbuf_display() {
        assert_eq!(format!("{}", RepoPathBuf::new()), "");
        assert_eq!(
            format!(
                "{}",
                RepoPathBuf::from_string(String::from("slice")).unwrap()
            ),
            "slice"
        );
        assert_eq!(
            format!("{}", RepoPathBuf::from_utf8(b"slice".to_vec()).unwrap()),
            "slice"
        );
    }

    #[test]
    fn test_pathbuf_debug() {
        assert_eq!(format!("{:?}", RepoPathBuf::new()), "RepoPathBuf(\"\")");
        assert_eq!(
            format!(
                "{:?}",
                RepoPathBuf::from_string(String::from("slice")).unwrap()
            ),
            "RepoPathBuf(\"slice\")"
        );
    }

    #[test]
    fn test_repo_path_conversions() {
        let repo_path_buf = RepoPathBuf::from_string(String::from("path_buf")).unwrap();
        assert_eq!(repo_path_buf.as_repo_path().to_owned(), repo_path_buf);

        let repo_path = RepoPath::from_str("path").unwrap();
        assert_eq!(repo_path.to_owned().as_repo_path(), repo_path);
    }

    #[test]
    fn test_repo_path_buf_push() {
        let mut repo_path_buf = RepoPathBuf::new();
        repo_path_buf.push(repo_path("one"));
        assert_eq!(repo_path_buf.as_repo_path(), repo_path("one"));
        repo_path_buf.push(repo_path("two"));
        assert_eq!(repo_path_buf.as_repo_path(), repo_path("one/two"));
    }

    #[test]
    fn test_repo_path_buf_pop() {
        let mut out = repo_path_buf("one/two/three");
        assert_eq!(out.pop(), Some(path_component_buf("three")));
        assert_eq!(out, repo_path_buf("one/two"));
        assert_eq!(out.pop(), Some(path_component_buf("two")));
        assert_eq!(out, repo_path_buf("one"));
        assert_eq!(out.pop(), Some(path_component_buf("one")));
        assert_eq!(out, RepoPathBuf::new());
        assert_eq!(out.pop(), None);
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
        assert_eq!(
            format!("{}", PathComponent::from_str("slice").unwrap()),
            "slice"
        );
    }

    #[test]
    fn test_component_debug() {
        assert_eq!(
            format!("{:?}", PathComponent::from_utf8(b"slice").unwrap()),
            "PathComponent(\"slice\")"
        );
        assert_eq!(
            format!("{:?}", PathComponent::from_str("slice").unwrap()),
            "PathComponent(\"slice\")"
        )
    }

    #[test]
    fn test_componentbuf_display() {
        assert_eq!(
            format!(
                "{}",
                PathComponentBuf::from_string(String::from("slice")).unwrap()
            ),
            "slice",
        );
    }

    #[test]
    fn test_componentbuf_debug() {
        assert_eq!(
            format!(
                "{:?}",
                PathComponentBuf::from_string(String::from("slice")).unwrap()
            ),
            "PathComponentBuf(\"slice\")"
        );
    }

    #[test]
    fn test_component_conversions() {
        let componentbuf = PathComponentBuf::from_string(String::from("componentbuf")).unwrap();
        assert_eq!(componentbuf.as_ref().to_owned(), componentbuf);

        let component = PathComponent::from_str("component").unwrap();
        assert_eq!(component.to_owned().as_ref(), component);
    }

    #[test]
    fn test_path_components() {
        let mut iter = repo_path("foo/bar/baz.txt").components();
        assert_eq!(iter.next().unwrap(), path_component("foo"));
        assert_eq!(iter.next().unwrap(), path_component("bar"));
        assert_eq!(iter.next().unwrap(), path_component("baz.txt"));
        assert!(iter.next().is_none());
    }

    #[test]
    fn test_append_component_to_path() {
        let expected = RepoPath::from_str("foo/bar/baz.txt").unwrap();
        let mut pathbuf = RepoPathBuf::new();
        for component in expected.components() {
            pathbuf.push(component);
        }
        assert_eq!(pathbuf.deref(), expected);
    }

    #[test]
    fn test_validate_path() {
        assert_eq!(
            format!("{}", validate_path("\n").unwrap_err()),
            "Invalid byte: 10."
        );
        assert_eq!(
            format!("{}", RepoPath::from_str("\n").unwrap_err()),
            "Failed to validate \"\\n\". Invalid byte: 10."
        );
        assert_eq!(
            format!("{}", validate_path("boo/").unwrap_err()),
            "Trailing slash."
        );
        assert_eq!(
            format!("{}", RepoPath::from_str("boo/").unwrap_err()),
            "Failed to validate \"boo/\". Trailing slash."
        );
    }

    #[test]
    fn test_validate_component() {
        assert_eq!(
            format!("{}", validate_component("foo/bar").unwrap_err()),
            "Invalid byte: 47."
        );
        assert_eq!(
            format!("{}", PathComponent::from_str("\n").unwrap_err()),
            "Failed to validate \"\\n\". Invalid byte: 10."
        );
        assert_eq!(
            format!("{}", validate_component("").unwrap_err()),
            "Invalid component: \"\"."
        );
        assert_eq!(
            format!("{}", PathComponent::from_str("").unwrap_err()),
            "Failed to validate \"\". Invalid component: \"\"."
        );
    }

    #[test]
    fn test_empty_path_components() {
        assert_eq!(RepoPathBuf::new().components().next(), None);
        assert_eq!(RepoPath::empty().components().next(), None);
    }

    #[test]
    fn test_empty_path_is_empty() {
        assert!(RepoPathBuf::new().is_empty());
        assert!(RepoPath::empty().is_empty());
    }

    #[test]
    fn test_parent() {
        assert_eq!(RepoPath::empty().parent(), None);
        assert_eq!(repo_path("foo").parent(), Some(RepoPath::empty()));
        assert_eq!(
            repo_path("foo/bar/baz").parent(),
            Some(repo_path("foo/bar"))
        );
    }

    #[test]
    fn test_last_component() {
        assert_eq!(RepoPath::empty().last_component(), None);
        assert_eq!(
            repo_path("foo").last_component(),
            Some(path_component("foo"))
        );
        assert_eq!(
            repo_path("foo/bar/baz").last_component(),
            Some(path_component("baz"))
        );
    }

    #[test]
    fn test_parents_on_regular_path() {
        let path = repo_path("foo/bar/baz/file.txt");
        let mut iter = path.parents();
        assert_eq!(iter.next(), Some(RepoPath::empty()));
        assert_eq!(iter.next(), Some(repo_path("foo")));
        assert_eq!(iter.next(), Some(repo_path("foo/bar")));
        assert_eq!(iter.next(), Some(repo_path("foo/bar/baz")));
        assert_eq!(iter.next(), None)
    }

    #[test]
    fn test_parents_on_empty_path() {
        assert_eq!(RepoPath::empty().parents().next(), None);
    }

    #[test]
    fn test_parents_and_components_in_parallel() {
        let path = repo_path("foo/bar/baz");
        let mut iter = path.parents().zip(path.components());
        assert_eq!(
            iter.next(),
            Some((RepoPath::empty(), path_component("foo")))
        );
        assert_eq!(iter.next(), Some((repo_path("foo"), path_component("bar"))));
        assert_eq!(
            iter.next(),
            Some((repo_path("foo/bar"), path_component("baz")))
        );
        assert_eq!(iter.next(), None);
    }

    quickcheck! {
       fn test_parents_equal_components(path: RepoPathBuf) -> bool {
           path.deref().parents().count() == path.deref().components().count()
        }
    }

    #[test]
    fn test_split_last_component() {
        assert_eq!(RepoPath::empty().split_last_component(), None);

        assert_eq!(
            repo_path("foo").split_last_component(),
            Some((RepoPath::empty(), path_component("foo")))
        );

        assert_eq!(
            repo_path("foo/bar/baz").split_last_component(),
            Some((repo_path("foo/bar"), path_component("baz")))
        );
    }

    #[test]
    fn test_to_owned() {
        assert_eq!(RepoPath::empty().to_owned(), RepoPathBuf::new());
        assert_eq!(repo_path("foo/bar").to_owned(), repo_path_buf("foo/bar"));
        assert_eq!(path_component("foo").to_owned(), path_component_buf("foo"));
    }

    #[test]
    fn test_sort_order() {
        assert!(RepoPath::empty() == RepoPath::empty());
        assert!(RepoPath::empty() < repo_path("foo"));
        assert!(repo_path("foo").cmp(RepoPath::empty()) == Ordering::Greater);
        assert!(repo_path("foo/bar") < repo_path("foo/baz"));
        assert!(repo_path("foo/bar") < repo_path("foo-bar"));
        assert!(repo_path("foo/bar") < repo_path("foobar"));
        assert!(repo_path("foo/bar") < repo_path("fooBar"));
        assert!(repo_path("foo/bar") < repo_path("fooo/bar"));
        assert!(repo_path("foo/bar") < repo_path("txt"));
        assert!(repo_path("foo/bar") == repo_path("foo/bar"));
        assert!(repo_path("foo/bar") > repo_path("bar"));
        assert!(repo_path("foo/bar") > repo_path("foo"));
    }
}
