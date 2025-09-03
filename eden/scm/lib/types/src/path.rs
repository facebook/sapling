/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
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

use std::borrow::Borrow;
use std::cmp::Ordering;
use std::fmt;
use std::ops::Deref;
use std::path::Path;
use std::path::PathBuf;
use std::str::Utf8Error;

use ref_cast::RefCastCustom;
use ref_cast::ref_cast_custom;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use thiserror::Error;

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
///
/// TODO: There is more validation that could be done here. Windows has a broad list of illegal
/// characters and reserved words.
///
/// It should be noted that `RepoPathBuf` and `RepoPath` implement `AsRef<RepoPath>`.
#[derive(Debug, Eq, PartialEq, Hash, RefCastCustom, Serialize)]
#[repr(transparent)]
pub struct RepoPath(str);

impl Default for &RepoPath {
    fn default() -> Self {
        RepoPath::empty()
    }
}

/// An owned version of a `PathComponent`. Not intended for mutation. RepoPathBuf is probably
/// more appropriate for mutation.
#[derive(
    Serialize,
    Deserialize,
    Clone,
    Debug,
    Default,
    Ord,
    PartialOrd,
    Eq,
    PartialEq,
    Hash
)]
#[serde(transparent)]
pub struct PathComponentBuf(String);

/// A `RepoPath` is a series of `PathComponent`s joined together by a separator (`/`).
/// Names for directories or files.
#[derive(Debug, Ord, PartialOrd, Eq, PartialEq, Hash, RefCastCustom)]
#[repr(transparent)]
pub struct PathComponent(str);

/// The One. The One Character We Use To Separate Paths Into Components.
pub const SEPARATOR: char = '/';
const SEPARATOR_BYTE: u8 = SEPARATOR as u8;

#[derive(Error, Debug)]
pub enum ParseError {
    ValidationError(String, ValidationError),
    InvalidUtf8(Vec<u8>, Utf8Error),
    // Prefer InvalidUtf8 if the actual UTF8 encoding is invalid. Use InvalidUnicode
    // if the string (e.g. OsString) cannot be interpreted as a valid sequence
    // of unicode codepoints. Accepts a lossy string conversion as its argument.
    InvalidUnicode(String),
}

impl ParseError {
    pub fn into_path_bytes(self) -> Vec<u8> {
        match self {
            ParseError::ValidationError(s, _) => s.into_bytes(),
            ParseError::InvalidUtf8(b, _) => b,
            ParseError::InvalidUnicode(s) => s.into_bytes(),
        }
    }
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
            ParseError::InvalidUnicode(lossy_string) => {
                write!(f, "Failed to parse 'unicode' string {:?}", lossy_string)
            }
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
    pub const fn new() -> RepoPathBuf {
        Self(String::new())
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

    /// Removed the last component from the `RepoPathBuf`, returning false if self was empty.
    pub fn pop(&mut self) -> bool {
        if self.0.is_empty() {
            return false;
        }

        self.0.truncate(self.0.rfind(SEPARATOR).unwrap_or(0));

        true
    }

    pub fn to_lower_case(&self) -> Self {
        Self(self.0.to_lowercase())
    }

    fn append(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }

        // Make sure we don't need two allocations below.
        self.0
            .reserve(s.len() + if self.0.is_empty() { 0 } else { 1 });

        if !self.0.is_empty() {
            self.0.push(SEPARATOR);
        }
        self.0.push_str(s);
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }
}

impl Ord for RepoPathBuf {
    fn cmp(&self, other: &RepoPathBuf) -> Ordering {
        self.as_repo_path().cmp(other.as_repo_path())
    }
}

impl PartialOrd for RepoPathBuf {
    fn partial_cmp(&self, other: &RepoPathBuf) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Deref for RepoPathBuf {
    type Target = RepoPath;
    fn deref(&self) -> &Self::Target {
        RepoPath::from_str_unchecked(&self.0)
    }
}

impl AsRef<RepoPath> for RepoPathBuf {
    fn as_ref(&self) -> &RepoPath {
        self
    }
}

impl AsRef<str> for RepoPathBuf {
    fn as_ref(&self) -> &str {
        &self.0
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
    pub fn from_utf8<S: AsRef<[u8]> + ?Sized>(s: &S) -> Result<&RepoPath, ParseError> {
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

    /// `const_fn` version of `from_str`.
    ///
    /// Path validation happens at compile time. For example, the code below
    /// will fail to compile because of the trailing slash:
    ///
    /// ```compile_fail
    /// # use types::RepoPath;
    /// static STATIC_PATH: &RepoPath = RepoPath::from_static_str("foo/");
    /// ```
    pub const fn from_static_str(s: &'static str) -> &'static RepoPath {
        if validate_path(s).is_err() {
            panic!("invalid RepoPath::from_static_str");
        }
        RepoPath::from_str_unchecked(s)
    }

    #[ref_cast_custom]
    const fn from_str_unchecked(s: &str) -> &RepoPath;

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

    /// Splits self into a head component and tail path. If self is empty, `None` is returned.
    /// If self has a single component, `Some((<component>, <empty path>))` is returned.
    pub fn split_first_component(&self) -> Option<(&PathComponent, &RepoPath)> {
        if self.is_empty() {
            return None;
        }
        match self.0.find(SEPARATOR) {
            Some(pos) => Some((
                PathComponent::from_str_unchecked(&self.0[..pos]),
                RepoPath::from_str_unchecked(&self.0[(pos + 1)..]),
            )),
            None => Some((
                PathComponent::from_str_unchecked(&self.0),
                RepoPath::empty(),
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
    /// It is possible to get iterate over parents with elements in parallel using:
    /// `path.parents().zip(path.components())`.
    pub fn parents(&'_ self) -> Parents<'_> {
        Parents::new(self)
    }

    /// Returns an iterator over the parents of the current path.
    /// The `RepoPath` itself is not returned. The root of the repository represented by the empty
    /// `RepoPath` is always returned by this iterator except if the path is empty.
    ///
    /// For example for the path `"foo/bar/baz"` this iterator will return three items:
    /// `"foo/bar"`, `"foo"`, and `""`.
    pub fn reverse_parents(&'_ self) -> ReverseParents<'_> {
        ReverseParents::new(self)
    }

    /// Returns an iterator over the components of the path.
    pub fn components(&'_ self) -> Components<'_> {
        Components::new(self)
    }

    /// Returns an iterator over the ancestors of the current path.
    ///
    /// Iterates closest ancestors first, including `self`.
    pub fn ancestors(&'_ self) -> Ancestors<'_> {
        Ancestors::new(self)
    }

    pub fn to_lower_case(&self) -> RepoPathBuf {
        RepoPathBuf(self.0.to_lowercase())
    }

    /// Create a std::Path::PathBuf from this RepoPath. "/" will be
    /// converted to the system path separator.
    pub fn to_path(&self) -> PathBuf {
        self.components().map(PathComponent::as_str).collect()
    }

    /// Return whether base's components prefix self's components.
    pub fn starts_with(&self, base: &Self, case_sensitive: bool) -> bool {
        self.strip_prefix(base, case_sensitive).is_some()
    }

    /// If `base` is a prefix of `self`, return suffix of `self`.
    pub fn strip_prefix(&self, base: &Self, case_sensitive: bool) -> Option<&Self> {
        self.strip(base, case_sensitive, true)
    }

    /// If `base` is a suffix of `self`, return prefix of `self`.
    pub fn strip_suffix(&self, base: &Self, case_sensitive: bool) -> Option<&Self> {
        self.strip(base, case_sensitive, false)
    }

    fn strip(&self, base: &Self, case_sensitive: bool, prefix: bool) -> Option<&Self> {
        if self.0.len() < base.0.len() {
            return None;
        }

        if base.is_empty() {
            return Some(self);
        }

        let (start, end) = if prefix {
            (0, base.0.len())
        } else {
            (self.0.len() - base.0.len(), self.0.len())
        };

        if start != 0 && self.0.as_bytes()[start - 1] != SEPARATOR_BYTE
            || end != self.0.len() && self.0.as_bytes()[end] != SEPARATOR_BYTE
        {
            return None;
        }

        let shared = &self.0[start..end];

        if shared == &base.0
            || (!case_sensitive
                && (shared.eq_ignore_ascii_case(&base.0)
                    || shared.to_lowercase() == base.0.to_lowercase()))
        {
            if self.0.len() == base.0.len() {
                Some(Self::empty())
            } else {
                Some(Self::from_str_unchecked(if prefix {
                    &self.0[end + 1..]
                } else {
                    &self.0[..start - 1]
                }))
            }
        } else {
            None
        }
    }

    /// Return common prefix of `self` and `other`.
    pub fn common_prefix<'a>(&'a self, other: &'a Self) -> &'a Self {
        let mut parts = self.components();
        let mut other_parts = other.components();

        loop {
            let position = parts.position;
            match (parts.next(), other_parts.next()) {
                (None, _) => return self,
                (_, None) => return other,
                (l, r) => {
                    if l != r {
                        return Self::from_str_unchecked(&self.0[..position.saturating_sub(1)]);
                    }
                }
            }
        }
    }

    /// Create a new RepoPathBuf joining self with other.
    pub fn join(&self, other: impl AsRef<RepoPath>) -> RepoPathBuf {
        let mut buf = self.to_owned();
        buf.push(other);
        buf
    }

    /// Return depth of self. Equivalent to `path.components().count()`, but faster.
    pub fn depth(&self) -> usize {
        if self.is_empty() {
            0
        } else {
            bytecount::count(self.0.as_bytes(), SEPARATOR_BYTE) + 1
        }
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

impl AsRef<str> for RepoPath {
    fn as_ref(&self) -> &str {
        &self.0
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

impl<'a> TryFrom<&'a str> for &'a RepoPath {
    type Error = ParseError;

    fn try_from(s: &'a str) -> Result<Self, Self::Error> {
        RepoPath::from_str(s)
    }
}

impl TryFrom<String> for RepoPathBuf {
    type Error = ParseError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        RepoPathBuf::from_string(s)
    }
}

impl PartialEq<RepoPathBuf> for &RepoPath {
    fn eq(&self, other: &RepoPathBuf) -> bool {
        *self == other.as_repo_path()
    }
}

/// Note: this is an anti-pattern and should generally not be used. However, some legacy code is
/// difficult to change and requires converting between the two types. This convenience method
/// allows the conversion to be done infalliably and with zero allocation.
impl From<PathComponentBuf> for RepoPathBuf {
    fn from(p: PathComponentBuf) -> RepoPathBuf {
        RepoPathBuf(p.into_string())
    }
}

impl TryFrom<PathBuf> for RepoPathBuf {
    type Error = ParseError;

    fn try_from(p: PathBuf) -> Result<Self, Self::Error> {
        RepoPathBuf::from_string(
            p.into_os_string().into_string().map_err(|os_str| {
                ParseError::InvalidUnicode(os_str.to_string_lossy().to_string())
            })?,
        )
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
        match validate_component(s.as_bytes()) {
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

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }
}

impl Deref for PathComponentBuf {
    type Target = PathComponent;
    fn deref(&self) -> &Self::Target {
        PathComponent::from_str_unchecked(&self.0)
    }
}

impl AsRef<PathComponent> for PathComponentBuf {
    fn as_ref(&self) -> &PathComponent {
        self
    }
}

impl AsRef<str> for PathComponentBuf {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl AsRef<[u8]> for PathComponentBuf {
    fn as_ref(&self) -> &[u8] {
        self.as_byte_slice()
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

impl<'a> TryFrom<&'a str> for &'a PathComponent {
    type Error = ParseError;

    fn try_from(s: &'a str) -> Result<Self, Self::Error> {
        PathComponent::from_str(s)
    }
}

impl TryFrom<String> for PathComponentBuf {
    type Error = ParseError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        PathComponentBuf::from_string(s)
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
        validate_component(s.as_bytes())
            .map_err(|e| ParseError::ValidationError(s.to_string(), e))?;
        Ok(PathComponent::from_str_unchecked(s))
    }

    /// `const_fn` version of `from_str`.
    ///
    /// Path validation happens at compile time.
    pub const fn from_static_str(s: &'static str) -> &'static PathComponent {
        if validate_component(s.as_bytes()).is_err() {
            panic!("invalid PathComponent::from_static_str");
        }
        Self::from_str_unchecked(s)
    }

    #[ref_cast_custom]
    const fn from_str_unchecked(s: &str) -> &PathComponent;

    /// Returns the underlying bytes of the `PathComponent`.
    pub fn as_byte_slice(&self) -> &[u8] {
        self.0.as_bytes()
    }

    /// Returns the `str` interpretation of the `PathComponent`.
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
        RepoPath::from_str_unchecked(&self.0)
    }
}

impl AsRef<RepoPath> for PathComponentBuf {
    fn as_ref(&self) -> &RepoPath {
        RepoPath::from_str_unchecked(&self.0)
    }
}

impl AsRef<[u8]> for PathComponent {
    fn as_ref(&self) -> &[u8] {
        self.as_byte_slice()
    }
}

impl AsRef<str> for PathComponent {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl PartialEq<PathComponentBuf> for PathComponent {
    fn eq(&self, other: &PathComponentBuf) -> bool {
        self == other.as_path_component()
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

const fn validate_path(s: &str) -> Result<(), ValidationError> {
    if s.is_empty() {
        return Ok(());
    }

    let bytes = s.as_bytes();
    if !bytes.is_empty() && bytes[bytes.len() - 1] == SEPARATOR_BYTE {
        return Err(ValidationError::TrailingSlash);
    }

    let mut i = 0;
    let mut start = 0;
    while i <= bytes.len() {
        if i == bytes.len() || bytes[i] == SEPARATOR_BYTE {
            let component = bytes.split_at(start).1.split_at(i - start).0;
            if let Err(e) = validate_component(component) {
                return Err(e);
            }
            start = i + 1;
        }
        i += 1;
    }

    Ok(())
}

const fn validate_component(bytes: &[u8]) -> Result<(), ValidationError> {
    match bytes.len() {
        0 => {
            return Err(ValidationError::InvalidPathComponent(
                InvalidPathComponent::Empty,
            ));
        }
        1 if bytes[0] == b'.' => {
            return Err(ValidationError::InvalidPathComponent(
                InvalidPathComponent::Current,
            ));
        }
        2 if bytes[0] == b'.' && bytes[1] == b'.' => {
            return Err(ValidationError::InvalidPathComponent(
                InvalidPathComponent::Parent,
            ));
        }
        _ => {}
    }

    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == 0u8 || b == 1u8 || b == b'\n' || b == b'\r' || b == b'/' {
            return Err(ValidationError::InvalidByte(b));
        }
        i += 1;
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

pub struct ReverseParents<'a> {
    path: &'a RepoPath,
    position: Option<usize>,
}

impl<'a> ReverseParents<'a> {
    pub fn new(path: &'a RepoPath) -> Self {
        ReverseParents {
            path,
            // Skip the last component since we only want the parents.
            position: if path.is_empty() {
                None
            } else {
                Some(path.0[..].rfind(SEPARATOR).unwrap_or(0))
            },
        }
    }
}

impl<'a> Iterator for ReverseParents<'a> {
    type Item = &'a RepoPath;

    fn next(&mut self) -> Option<Self::Item> {
        match self.position {
            Some(ref mut position) if *position != 0 => {
                let result = RepoPath::from_str_unchecked(&self.path.0[0..*position]);
                *position = self.path.0[..*position].rfind(SEPARATOR).unwrap_or(0);
                Some(result)
            }
            Some(_) => {
                self.position = None;
                Some(RepoPath::empty())
            }
            None => None,
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

pub struct Ancestors<'a> {
    path: Option<&'a RepoPath>,
}

impl<'a> Ancestors<'a> {
    pub fn new(path: &'a RepoPath) -> Self {
        Ancestors { path: Some(path) }
    }
}

impl<'a> Iterator for Ancestors<'a> {
    type Item = &'a RepoPath;

    fn next(&mut self) -> Option<Self::Item> {
        let path = self.path;
        self.path = path.and_then(RepoPath::parent);
        path
    }
}

enum RepoPathRelativizerConfig {
    // If the cwd is inside the repo, then Hg paths should be relativized against the cwd relative
    // to the repo root.
    CwdUnderRepo { relative_cwd: PathBuf },

    // If the cwd is outside the repo, then prefix is the cwd relative to the repo root: Hg paths
    // can simply be appended to this path.
    CwdOutsideRepo { prefix: PathBuf },
}

pub struct RepoPathRelativizer {
    config: RepoPathRelativizerConfig,
}

/// Utility for computing a relativized path for a file in an Hg repository given the user's cwd
/// and specified value for --repository/-R, if any.
///
/// Note: the caller is responsible for normalizing the repo_root and cwd parameters ahead of time.
/// If these are specified in different formats (e.g., on Windows one is a UNC path and the other
/// is not), then this function will not produce expected results.  Unfortunately the Rust library
/// does not provide a mechanism for normalizing paths.  The best thing for callers to do for now
/// is probably to call Path::canonicalize() if they expect that the paths do exist on disk.
impl RepoPathRelativizer {
    /// `cwd` corresponds to getcwd(2) while `repo_root` is the absolute path specified via
    /// --repository/-R, or failing that, the Hg repo that contains `cwd`.
    pub fn new(cwd: impl AsRef<Path>, repo_root: impl AsRef<Path>) -> Self {
        RepoPathRelativizer::new_impl(cwd.as_ref(), repo_root.as_ref())
    }

    fn new_impl(cwd: &Path, repo_root: &Path) -> Self {
        use self::RepoPathRelativizerConfig::*;
        let config = if cwd.starts_with(repo_root) {
            CwdUnderRepo {
                relative_cwd: util::path::relativize(repo_root, cwd),
            }
        } else {
            CwdOutsideRepo {
                prefix: util::path::relativize(cwd, repo_root),
            }
        };
        RepoPathRelativizer { config }
    }

    /// Relativize the [`RepoPath`]. Returns a String that is suitable for display to the user.
    pub fn relativize(&self, path: impl AsRef<RepoPath>) -> String {
        fn inner(relativizer: &RepoPathRelativizer, path: &RepoPath) -> String {
            // TODO: directly operate on the RepoPath components.
            let path = path.to_path();

            use self::RepoPathRelativizerConfig::*;
            let output = match &relativizer.config {
                CwdUnderRepo { relative_cwd } => util::path::relativize(relative_cwd, &path),
                CwdOutsideRepo { prefix } => prefix.join(path),
            };
            output.display().to_string()
        }

        inner(self, path.as_ref())
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl quickcheck::Arbitrary for RepoPathBuf {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        let size = usize::arbitrary(g) % 8;
        let mut path_buf = RepoPathBuf::new();
        for _ in 0..size {
            path_buf.push(PathComponentBuf::arbitrary(g).as_path_component());
        }
        path_buf
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl quickcheck::Arbitrary for PathComponentBuf {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        // Most strings should be valid `PathComponent` so it is reasonable to loop until a valid
        // string is found. To note that generating Arbitrary Unicode on `char` is implemented
        // using a loop where random bytes are validated against the `char` constructor.
        loop {
            let size = usize::arbitrary(g) % 7 + 1;
            let mut s = String::with_capacity(size);
            for _ in 0..size {
                let x = (u64::arbitrary(g) % 25) as u8;
                let x = (b'a' + x) as char;
                s.push(x);
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
    use lazy_static::lazy_static;

    use super::*;

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
    use quickcheck::quickcheck;

    use super::*;
    use crate::testutil::path_component;
    use crate::testutil::path_component_buf;
    use crate::testutil::repo_path;
    use crate::testutil::repo_path_buf;

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
        assert!(out.pop());
        assert_eq!(out, repo_path_buf("one/two"));
        assert!(out.pop());
        assert_eq!(out, repo_path_buf("one"));
        assert!(out.pop());
        assert_eq!(out, RepoPathBuf::new());
        assert!(!out.pop());
    }

    #[test]
    fn test_component_initialization_with_invalid_utf8() {
        assert!(PathComponent::from_utf8(&[0x80, 0x80]).is_err());
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
        assert_eq!(componentbuf.as_path_component().to_owned(), componentbuf);

        let component = PathComponent::from_str("component").unwrap();
        assert_eq!(component.to_owned().as_path_component(), component);
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
        let from_str = |s: &'static str| {
            let result = RepoPath::from_str(s);
            if let Ok(path1) = &result {
                let path2 = RepoPath::from_static_str(s);
                assert_eq!(*path1, path2);
            }
            result
        };
        assert_eq!(
            format!("{}", validate_path("\n").unwrap_err()),
            "Invalid byte: 10."
        );
        assert_eq!(
            format!("{}", validate_path("\r").unwrap_err()),
            "Invalid byte: 13."
        );
        assert_eq!(
            format!("{}", from_str("\n").unwrap_err()),
            "Failed to validate \"\\n\". Invalid byte: 10."
        );
        assert_eq!(
            format!("{}", validate_path("boo/").unwrap_err()),
            "Trailing slash."
        );
        assert_eq!(
            format!("{}", from_str("boo/").unwrap_err()),
            "Failed to validate \"boo/\". Trailing slash."
        );
    }

    #[test]
    fn test_const_repo_path() {
        const PATH_STR: &str = "foo/bar";
        static STATIC_PATH: &RepoPath = RepoPath::from_static_str(PATH_STR);
        assert_eq!(STATIC_PATH.as_str(), PATH_STR);
    }

    #[test]
    fn test_validate_component() {
        assert_eq!(
            format!("{}", validate_component(b"foo/bar").unwrap_err()),
            "Invalid byte: 47."
        );
        assert_eq!(
            format!("{}", PathComponent::from_str("\n").unwrap_err()),
            "Failed to validate \"\\n\". Invalid byte: 10."
        );
        assert_eq!(
            format!("{}", validate_component(b"").unwrap_err()),
            "Invalid component: \"\"."
        );
        assert_eq!(
            format!("{}", PathComponent::from_str("").unwrap_err()),
            "Failed to validate \"\". Invalid component: \"\"."
        );
    }

    #[test]
    fn test_const_path_component() {
        const COMPONENT_STR: &str = "foo";
        static STATIC_COMPONENT: &PathComponent = PathComponent::from_static_str(COMPONENT_STR);
        assert_eq!(STATIC_COMPONENT.as_str(), COMPONENT_STR);
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
    fn test_reverse_parents_on_regular_path() {
        let path = repo_path("foo/bar/baz/file.txt");
        let mut iter = path.reverse_parents();
        assert_eq!(iter.next(), Some(repo_path("foo/bar/baz")));
        assert_eq!(iter.next(), Some(repo_path("foo/bar")));
        assert_eq!(iter.next(), Some(repo_path("foo")));
        assert_eq!(iter.next(), Some(RepoPath::empty()));
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

    #[test]
    fn test_ancestors_on_regular_path() {
        let path = repo_path("foo/bar/baz/file.txt");
        let mut iter = path.ancestors();
        assert_eq!(iter.next(), Some(repo_path("foo/bar/baz/file.txt")));
        assert_eq!(iter.next(), Some(repo_path("foo/bar/baz")));
        assert_eq!(iter.next(), Some(repo_path("foo/bar")));
        assert_eq!(iter.next(), Some(repo_path("foo")));
        assert_eq!(iter.next(), Some(RepoPath::empty()));
        assert_eq!(iter.next(), None)
    }

    #[test]
    fn test_ancestors_on_empty_path() {
        let mut iter = RepoPath::empty().ancestors();
        assert_eq!(iter.next(), Some(repo_path("")));
        assert_eq!(iter.next(), None);
    }

    quickcheck! {
       fn test_parents_equal_components(path: RepoPathBuf) -> bool {
           path.deref().parents().count() == path.deref().components().count()
        }

       fn test_ancestors_relates_to_parents(path: RepoPathBuf) -> bool {
           path.deref().ancestors().count() == path.deref().parents().count() + 1
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
    fn test_split_first_component() {
        assert_eq!(RepoPath::empty().split_first_component(), None);

        assert_eq!(
            repo_path("foo").split_first_component(),
            Some((path_component("foo"), RepoPath::empty()))
        );

        assert_eq!(
            repo_path("foo/bar/baz").split_first_component(),
            Some((path_component("foo"), repo_path("bar/baz")))
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

    // Convert to ["foo", "bar"] to "foo\bar" on Windows, else "foo/bar".
    fn os_path(parts: &[&str]) -> String {
        parts.iter().collect::<PathBuf>().to_string_lossy().into()
    }

    #[test]
    fn test_relativize_path_from_repo_when_cwd_is_repo_root() {
        let repo_root = Path::new("/home/zuck/tfb");
        let cwd = Path::new("/home/zuck/tfb");
        let relativizer = RepoPathRelativizer::new(cwd, repo_root);
        let check = |path, expected| {
            assert_eq!(relativizer.relativize(repo_path(path)), expected);
        };
        check("foo/bar.txt", os_path(&["foo", "bar.txt"]));
    }

    #[test]
    fn test_relativize_path_from_repo_when_cwd_is_descendant_of_repo_root() {
        let repo_root = Path::new("/home/zuck/tfb");
        let cwd = Path::new("/home/zuck/tfb/foo");
        let relativizer = RepoPathRelativizer::new(cwd, repo_root);
        let check = |path, expected| {
            assert_eq!(relativizer.relativize(repo_path(path)), expected);
        };
        check("foo/bar.txt", "bar.txt");
    }

    #[test]
    fn test_relativize_path_from_repo_when_cwd_is_ancestor_of_repo_root() {
        let repo_root = PathBuf::from("/home/zuck/tfb");
        let cwd = PathBuf::from("/home/zuck");
        let relativizer = RepoPathRelativizer::new(cwd, repo_root);
        let check = |path, expected| {
            assert_eq!(relativizer.relativize(repo_path(path)), expected);
        };
        check("foo/bar.txt", os_path(&["tfb", "foo", "bar.txt"]));
    }

    #[test]
    fn test_relativize_path_from_repo_when_cwd_is_cousin_of_repo_root() {
        let relativizer = RepoPathRelativizer::new("/home/schrep/tfb", "/home/zuck/tfb");
        let check = |path, expected| {
            assert_eq!(relativizer.relativize(repo_path(path)), expected);
        };
        check(
            "foo/bar.txt",
            os_path(&["..", "..", "zuck", "tfb", "foo", "bar.txt"]),
        );
    }

    #[test]
    fn test_starts_with() {
        assert!(repo_path("").starts_with(repo_path(""), true));
        assert!(repo_path("").starts_with(repo_path(""), false));

        assert!(!repo_path("").starts_with(repo_path("foo"), true));
        assert!(!repo_path("").starts_with(repo_path("foo"), false));

        assert!(repo_path("foo").starts_with(repo_path(""), true));
        assert!(repo_path("foo").starts_with(repo_path(""), false));

        assert!(repo_path("foo").starts_with(repo_path("foo"), true));
        assert!(repo_path("foo").starts_with(repo_path("foo"), false));

        assert!(!repo_path("foobar").starts_with(repo_path("foo"), true));
        assert!(!repo_path("foobar").starts_with(repo_path("foo"), false));

        assert!(repo_path("foo/bar/baz").starts_with(repo_path("foo/bar"), true));
        assert!(!repo_path("foo/bAr/baz").starts_with(repo_path("foo/bar"), true));
        assert!(repo_path("foo/bAr/baz").starts_with(repo_path("foo/bar"), false));
        assert!(repo_path("foo/bÄr/baz").starts_with(repo_path("foo/bär"), false));

        assert!(!repo_path("foo/bar/baz").starts_with(repo_path("foo/bar/baz/qux"), true));
        assert!(!repo_path("foo/bar/baz").starts_with(repo_path("foo/bar/baz/qux"), false));
    }

    #[test]
    fn test_strip_prefix() {
        assert_eq!(
            repo_path("").strip_prefix(repo_path(""), true),
            Some(repo_path(""))
        );
        assert_eq!(
            repo_path("").strip_prefix(repo_path(""), false),
            Some(repo_path(""))
        );

        assert!(repo_path("").strip_prefix(repo_path("foo"), true).is_none());
        assert!(
            repo_path("")
                .strip_prefix(repo_path("foo"), false)
                .is_none()
        );

        assert_eq!(
            repo_path("foo").strip_prefix(repo_path(""), true),
            Some(repo_path("foo"))
        );
        assert_eq!(
            repo_path("foo").strip_prefix(repo_path(""), false),
            Some(repo_path("foo"))
        );

        assert_eq!(
            repo_path("foo").strip_prefix(repo_path("foo"), true),
            Some(repo_path(""))
        );
        assert_eq!(
            repo_path("foo").strip_prefix(repo_path("foo"), false),
            Some(repo_path(""))
        );

        assert!(
            repo_path("foobar")
                .strip_prefix(repo_path("foo"), true)
                .is_none()
        );
        assert!(
            repo_path("foobar")
                .strip_prefix(repo_path("foo"), false)
                .is_none()
        );

        assert_eq!(
            repo_path("foo/bar/baz").strip_prefix(repo_path("foo/bar"), true),
            Some(repo_path("baz"))
        );
        assert!(
            repo_path("foo/bAr/baz")
                .strip_prefix(repo_path("foo/bar"), true)
                .is_none()
        );
        assert_eq!(
            repo_path("foo/bAr/baz").strip_prefix(repo_path("foo/bar"), false),
            Some(repo_path("baz"))
        );
        assert_eq!(
            repo_path("foo/bÄr/baz").strip_prefix(repo_path("foo/bär"), false),
            Some(repo_path("baz"))
        );

        assert!(
            repo_path("foo/bar/baz")
                .strip_prefix(repo_path("foo/bar/baz/qux"), true)
                .is_none()
        );
        assert!(
            repo_path("foo/bar/baz")
                .strip_prefix(repo_path("foo/bar/baz/qux"), false)
                .is_none()
        );
    }

    #[test]
    fn test_strip_suffix() {
        assert_eq!(
            repo_path("").strip_suffix(repo_path(""), true),
            Some(repo_path(""))
        );
        assert_eq!(
            repo_path("").strip_suffix(repo_path(""), false),
            Some(repo_path(""))
        );

        assert!(repo_path("").strip_suffix(repo_path("foo"), true).is_none());
        assert!(
            repo_path("")
                .strip_suffix(repo_path("foo"), false)
                .is_none()
        );

        assert_eq!(
            repo_path("foo").strip_suffix(repo_path(""), true),
            Some(repo_path("foo"))
        );
        assert_eq!(
            repo_path("foo").strip_suffix(repo_path(""), false),
            Some(repo_path("foo"))
        );

        assert_eq!(
            repo_path("foo").strip_suffix(repo_path("foo"), true),
            Some(repo_path(""))
        );
        assert_eq!(
            repo_path("foo").strip_suffix(repo_path("foo"), false),
            Some(repo_path(""))
        );

        assert!(
            repo_path("foobar")
                .strip_suffix(repo_path("bar"), true)
                .is_none()
        );
        assert!(
            repo_path("foobar")
                .strip_suffix(repo_path("bar"), false)
                .is_none()
        );

        assert_eq!(
            repo_path("foo/bar/baz").strip_suffix(repo_path("bar/baz"), true),
            Some(repo_path("foo"))
        );
        assert!(
            repo_path("foo/bAr/baz")
                .strip_suffix(repo_path("bar/baz"), true)
                .is_none()
        );
        assert_eq!(
            repo_path("foo/bAr/baz").strip_suffix(repo_path("bar/baz"), false),
            Some(repo_path("foo"))
        );
        assert_eq!(
            repo_path("foo/bÄr/baz").strip_suffix(repo_path("bär/baz"), false),
            Some(repo_path("foo"))
        );

        assert!(
            repo_path("foo/bar/baz")
                .strip_suffix(repo_path("foo/bar/baz/qux"), true)
                .is_none()
        );
        assert!(
            repo_path("foo/bar/baz")
                .strip_suffix(repo_path("foo/bar/baz/qux"), false)
                .is_none()
        );
    }

    #[test]
    fn test_common_prefix() {
        assert_eq!(repo_path("").common_prefix(repo_path("foo")), repo_path(""));
        assert_eq!(repo_path("foo").common_prefix(repo_path("")), repo_path(""));
        assert_eq!(
            repo_path("foo").common_prefix(repo_path("foobar")),
            repo_path("")
        );
        assert_eq!(
            repo_path("foo").common_prefix(repo_path("foo")),
            repo_path("foo")
        );
        assert_eq!(
            repo_path("foo/bar").common_prefix(repo_path("foo")),
            repo_path("foo")
        );
        assert_eq!(
            repo_path("foo").common_prefix(repo_path("foo/bar")),
            repo_path("foo")
        );
        assert_eq!(
            repo_path("foo/bar").common_prefix(repo_path("foo/bar")),
            repo_path("foo/bar")
        );
        assert_eq!(
            repo_path("foo/bar/baz/qux").common_prefix(repo_path("bar")),
            repo_path("")
        );
        assert_eq!(
            repo_path("foo/bar/baz/qux").common_prefix(repo_path("foo/bar")),
            repo_path("foo/bar")
        );
        assert_eq!(
            repo_path("foo/bar").common_prefix(repo_path("foo/bar/baz/qux")),
            repo_path("foo/bar")
        );
    }

    #[test]
    fn test_join_empty() {
        let p = repo_path("foo");
        assert_eq!(p.join(repo_path("")), repo_path_buf("foo"));
    }

    #[test]
    fn test_depth() {
        fn check(p: &RepoPath, expected: usize) {
            assert_eq!(p.depth(), expected);
            assert_eq!(p.components().count(), expected);
        }

        check(repo_path(""), 0);
        check(repo_path("foo"), 1);
        check(repo_path("foo/bar"), 2);
    }
}
