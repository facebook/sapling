/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cmp;
use std::collections::HashMap;
use std::fmt;
use std::fmt::Display;
use std::io;
use std::io::Write;
use std::iter::once;
use std::iter::Once;
use std::slice::Iter;

use anyhow::bail;
use anyhow::Context as _;
use anyhow::Error;
use anyhow::Result;
use ascii::AsciiString;
use lazy_static::lazy_static;
use quickcheck::Arbitrary;
use quickcheck::Gen;
use quickcheck_arbitrary_derive::Arbitrary;
use regex::bytes::Regex as BytesRegex;
use regex::Regex;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use smallvec::SmallVec;

use crate::bonsai_changeset::BonsaiChangeset;
use crate::errors::ErrorKind;
use crate::hash::Blake2;
use crate::hash::Context;
use crate::thrift;

// Filesystems on Linux commonly limit path *elements* to 255 bytes. Enforce this on MPaths as well
// as a repository that cannot be checked out isn't very useful.
const MPATH_ELEMENT_MAX_LENGTH: usize = 255;

/// A path or filename within Mononoke, with information about whether
/// it's the root of the repo, a directory or a file.
#[derive(Arbitrary, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RepoPath {
    // It is now *completely OK* to create a RepoPath directly. All MPaths are valid once
    // constructed.
    RootPath,
    DirectoryPath(MPath),
    FilePath(MPath),
}

impl RepoPath {
    #[inline]
    pub fn root() -> Self {
        RepoPath::RootPath
    }

    pub fn dir<P>(path: P) -> Result<Self>
    where
        P: TryInto<MPath>,
        Error: From<P::Error>,
    {
        let path = path.try_into()?;
        Ok(RepoPath::DirectoryPath(path))
    }

    pub fn file<P>(path: P) -> Result<Self>
    where
        P: TryInto<MPath>,
        Error: From<P::Error>,
    {
        let path = path.try_into()?;
        Ok(RepoPath::FilePath(path))
    }

    /// Whether this path represents the root.
    #[inline]
    pub fn is_root(&self) -> bool {
        match *self {
            RepoPath::RootPath => true,
            _ => false,
        }
    }

    /// Whether this path represents a directory that isn't the root.
    #[inline]
    pub fn is_dir(&self) -> bool {
        match *self {
            RepoPath::DirectoryPath(_) => true,
            _ => false,
        }
    }

    /// Whether this patch represents a tree (root or other directory).
    #[inline]
    pub fn is_tree(&self) -> bool {
        match *self {
            RepoPath::RootPath => true,
            RepoPath::DirectoryPath(_) => true,
            _ => false,
        }
    }

    /// Whether this path represents a file.
    #[inline]
    pub fn is_file(&self) -> bool {
        match *self {
            RepoPath::FilePath(_) => true,
            _ => false,
        }
    }

    #[allow(clippy::len_without_is_empty)]
    /// Get the length of this repo path in bytes. `RepoPath::Root` has length 0.
    pub fn len(&self) -> usize {
        match *self {
            RepoPath::RootPath => 0,
            RepoPath::DirectoryPath(ref path) => path.len(),
            RepoPath::FilePath(ref path) => path.len(),
        }
    }

    pub fn mpath(&self) -> Option<&MPath> {
        match *self {
            RepoPath::RootPath => None,
            RepoPath::DirectoryPath(ref path) => Some(path),
            RepoPath::FilePath(ref path) => Some(path),
        }
    }

    pub fn into_mpath(self) -> Option<MPath> {
        match self {
            RepoPath::RootPath => None,
            RepoPath::DirectoryPath(path) => Some(path),
            RepoPath::FilePath(path) => Some(path),
        }
    }

    /// Serialize this RepoPath into a string. This shouldn't (yet) be considered stable if the
    /// definition of RepoPath changes.
    pub fn serialize(&self) -> Vec<u8> {
        bincode::serialize(self).expect("serialize for RepoPath cannot fail")
    }

    /// Serialize this RepoPath into a writer. This shouldn't (yet) be considered stable if the
    /// definition of RepoPath changes.
    pub fn serialize_into<W: Write>(&self, writer: &mut W) -> Result<()> {
        Ok(bincode::serialize_into(writer, self)?)
    }

    pub fn from_thrift(path: thrift::RepoPath) -> Result<Self> {
        let path = match path {
            thrift::RepoPath::RootPath(_) => Self::root(),
            thrift::RepoPath::DirectoryPath(path) => Self::dir(MPath::from_thrift(path)?)?,
            thrift::RepoPath::FilePath(path) => Self::file(MPath::from_thrift(path)?)?,
            thrift::RepoPath::UnknownField(unknown) => bail!(
                "Unknown field encountered when parsing thrift::RepoPath: {}",
                unknown,
            ),
        };
        Ok(path)
    }

    pub fn into_thrift(self) -> thrift::RepoPath {
        match self {
            // dummy false here is required because thrift doesn't support mixing enums with and
            // without payload
            RepoPath::RootPath => thrift::RepoPath::RootPath(false),
            RepoPath::DirectoryPath(path) => {
                thrift::RepoPath::DirectoryPath(MPath::into_thrift(path))
            }
            RepoPath::FilePath(path) => thrift::RepoPath::FilePath(MPath::into_thrift(path)),
        }
    }
}

impl Display for RepoPath {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            RepoPath::RootPath => write!(f, "(root path)"),
            RepoPath::DirectoryPath(ref path) => write!(f, "directory '{}'", path),
            RepoPath::FilePath(ref path) => write!(f, "file '{}'", path),
        }
    }
}

/// This trait impl allows passing in a &RepoPath where `Into<RepoPath>` is requested.
impl<'a> From<&'a RepoPath> for RepoPath {
    fn from(path: &'a RepoPath) -> RepoPath {
        path.clone()
    }
}

/// An element of a path or filename within Mercurial.
///
/// Mercurial treats pathnames as sequences of bytes, but the manifest format
/// assumes they cannot contain zero bytes. The bytes are not necessarily utf-8
/// and so cannot be converted into a string (or - strictly speaking - be displayed).
///
/// Internally using SmallVec as many path elements are directory names and thus
/// quite short, avoiding need for heap alloc. Its stack storage size is set to 24
/// as with the union feature the smallvec is 32 bytes on stack which is same as previous
/// Bytes member stack sise (Bytes will usually have heap as well of course)
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct MPathElement(SmallVec<[u8; 24]>);

impl MPathElement {
    #[inline]
    pub fn new(element: Vec<u8>) -> Result<MPathElement> {
        Self::verify(&element)?;
        Ok(MPathElement(SmallVec::from(element)))
    }

    #[inline]
    pub fn from_smallvec(element: SmallVec<[u8; 24]>) -> Result<MPathElement> {
        Self::verify(&element)?;
        Ok(MPathElement(element))
    }

    #[inline]
    pub fn new_from_slice(element: &[u8]) -> Result<MPathElement> {
        Self::verify(element)?;
        Ok(MPathElement(SmallVec::from(element)))
    }

    #[inline]
    pub fn from_thrift(element: thrift::MPathElement) -> Result<MPathElement> {
        Self::verify(&element.0).with_context(|| {
            ErrorKind::InvalidThrift("MPathElement".into(), "invalid path element".into())
        })?;
        Ok(MPathElement(element.0))
    }

    fn verify(p: &[u8]) -> Result<()> {
        if p.is_empty() {
            bail!(ErrorKind::InvalidPath(
                "".into(),
                "path elements cannot be empty".into()
            ));
        }
        if p.contains(&0) {
            bail!(ErrorKind::InvalidPath(
                String::from_utf8_lossy(p).into_owned(),
                "path elements cannot contain '\\0'".into(),
            ));
        }
        if p.contains(&1) {
            // MPath can not contain '\x01', in particular if mpath ends with '\x01'
            // and it is part of move metadata, because key-value pairs are separated
            // by '\n', you will get '\x01\n' which is also metadata separator.
            bail!(ErrorKind::InvalidPath(
                String::from_utf8_lossy(p).into_owned(),
                "path elements cannot contain '\\1'".into(),
            ));
        }
        if p.contains(&b'/') {
            bail!(ErrorKind::InvalidPath(
                String::from_utf8_lossy(p).into_owned(),
                "path elements cannot contain '/'".into(),
            ));
        }
        if p.contains(&b'\n') {
            bail!(ErrorKind::InvalidPath(
                String::from_utf8_lossy(p).into_owned(),
                "path elements cannot contain '\\n'".into(),
            ));
        }
        if p == b"." || p == b".." {
            bail!(ErrorKind::InvalidPath(
                String::from_utf8_lossy(p).into_owned(),
                "path elements cannot be . or .. to avoid traversal attacks".into(),
            ));
        }
        Self::check_len(p)?;
        Ok(())
    }

    fn check_len(p: &[u8]) -> Result<()> {
        if p.len() > MPATH_ELEMENT_MAX_LENGTH {
            bail!(ErrorKind::InvalidPath(
                String::from_utf8_lossy(p).into_owned(),
                format!(
                    "path elements cannot exceed {} bytes",
                    MPATH_ELEMENT_MAX_LENGTH
                )
            ));
        }

        Ok(())
    }

    #[allow(clippy::len_without_is_empty)]
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn into_thrift(self) -> thrift::MPathElement {
        thrift::MPathElement(self.0)
    }

    /// Returns true if this path element is valid UTF-8.
    pub fn is_utf8(&self) -> bool {
        std::str::from_utf8(self.0.as_ref()).is_ok()
    }

    /// Returns the length of the path element in WCHARs, if the path element
    /// is re-interpreted as a Windows filename.
    ///
    /// For UTF-8 path elements, this is the length of the UTF-16 encoding.
    /// For other path elementss, it is assumed that a Windows 8-bit encoding
    /// is in use and each byte corresponds to one WCHAR.
    pub fn wchar_len(&self) -> usize {
        match std::str::from_utf8(self.0.as_ref()) {
            Ok(s) => s.encode_utf16().count(),
            Err(_) => self.0.len(),
        }
    }

    /// Returns the lowercased version of this MPath element if it is valid
    /// UTF-8.
    pub fn to_lowercase_utf8(&self) -> Option<String> {
        let s = std::str::from_utf8(self.0.as_ref()).ok()?;
        let s = s.to_lowercase();
        Some(s)
    }

    /// Returns whether this path element is a valid filename on Windows.
    /// ```text
    ///
    /// Invalid filenames on Windows are:
    ///
    /// * Any filename containing a control character in the range 0-31, or
    ///   any character in the set `< > : " / \\ | ? *`.
    /// * Any filename ending in a `.` or a space.
    /// * Any filename that is `CON`, `PRN`, `AUX`, `NUL`, `COM1-9` or
    ///   `LPT1-9`, with or without an extension.
    /// ```
    pub fn is_valid_windows_filename(&self) -> bool {
        // File names containing any of <>:"/\|?* or control characters are invalid.
        let is_invalid = |c: &u8| *c < b' ' || b"<>:\"/\\|?*".iter().any(|i| i == c);
        if self.0.iter().any(is_invalid) {
            return false;
        }

        // File names ending in . or space are invalid.
        if let Some(b' ') | Some(b'.') = self.0.last() {
            return false;
        }

        // CON, PRN, AUX, NUL, COM[1-9] and LPT[1-9] are invalid, with or
        // without extension.
        if INVALID_WINDOWS_FILENAME_REGEX.is_match(self.0.as_ref()) {
            return false;
        }

        true
    }

    /// Returns whether potential_suffix is a suffix of this path element.
    /// For example, if the element is "file.extension", "n", "tension",
    /// "extension", ".extension", "file.extension" are suffixes of the
    /// basename, but "file" is not.
    #[inline]
    pub fn has_suffix(&self, potential_suffix: &[u8]) -> bool {
        self.0.ends_with(potential_suffix)
    }

    #[inline]
    pub fn starts_with(&self, prefix: &[u8]) -> bool {
        self.0.starts_with(prefix)
    }

    /// Reverse this path element inplace
    pub fn reverse(&mut self) {
        self.0.reverse()
    }
}

// Regex for looking for invalid windows filenames
lazy_static! {
    static ref INVALID_WINDOWS_FILENAME_REGEX: BytesRegex =
        BytesRegex::new("^((?i)CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])([.][^.]*|)$")
            .expect("invalid windows filename regex should be valid");
}

impl AsRef<[u8]> for MPathElement {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl From<MPathElement> for MPath {
    fn from(element: MPathElement) -> Self {
        MPath {
            elements: vec![element],
        }
    }
}

/// A path or filename within Mononoke (typically within manifests or changegroups).
///
/// This is called `MPath` so that it can be differentiated from `std::path::Path`.
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash)]
#[derive(Serialize, Deserialize)]
pub struct MPath {
    elements: Vec<MPathElement>,
}

impl Extend<MPathElement> for Option<MPath> {
    fn extend<T: IntoIterator<Item = MPathElement>>(&mut self, iter: T) {
        match self {
            Some(ref mut path) => {
                path.elements.extend(iter);
            }
            None => {
                let elements = Vec::from_iter(iter);
                if elements.is_empty() {
                    *self = None;
                } else {
                    *self = Some(MPath { elements });
                }
            }
        }
    }
}

impl MPath {
    pub fn new<P: AsRef<[u8]>>(p: P) -> Result<MPath> {
        let p = p.as_ref();
        let elements: Vec<_> = p
            .split(|c| *c == b'/')
            .filter(|e| !e.is_empty())
            .map(MPathElement::new_from_slice)
            .collect::<Result<_, _>>()?;
        if elements.is_empty() {
            bail!(ErrorKind::InvalidPath(
                String::from_utf8_lossy(p).into_owned(),
                "path cannot be empty".into()
            ));
        }
        Ok(MPath { elements })
    }

    /// Same as `MPath::new`, except the input bytes may be empty.
    pub fn new_opt<P: AsRef<[u8]>>(p: P) -> Result<Option<MPath>> {
        let p = p.as_ref();
        if p.is_empty() {
            Ok(None)
        } else {
            Ok(Some(MPath::new(p)?))
        }
    }

    pub fn from_thrift(mpath: thrift::MPath) -> Result<MPath> {
        let elements: Result<Vec<_>> = mpath.0.into_iter().map(MPathElement::from_thrift).collect();
        let elements = elements?;

        if elements.is_empty() {
            bail!("Unexpected empty path in thrift::MPath")
        } else {
            Ok(MPath { elements })
        }
    }

    pub fn join<'a, Elements: IntoIterator<Item = &'a MPathElement>>(
        &self,
        another: Elements,
    ) -> MPath {
        let mut newelements = self.elements.clone();
        newelements.extend(
            another
                .into_iter()
                .filter(|elem| !elem.0.is_empty())
                .cloned(),
        );
        MPath {
            elements: newelements,
        }
    }

    pub fn join_element(&self, element: Option<&MPathElement>) -> MPath {
        match element {
            Some(element) => self.join(element),
            None => self.clone(),
        }
    }

    pub fn join_opt<'a, Elements: IntoIterator<Item = &'a MPathElement>>(
        path: Option<&Self>,
        another: Elements,
    ) -> Option<Self> {
        match path {
            Some(path) => Some(path.join(another)),
            None => {
                let elements: Vec<MPathElement> = another
                    .into_iter()
                    .filter(|elem| !elem.0.is_empty())
                    .cloned()
                    .collect();
                if elements.is_empty() {
                    None
                } else {
                    Some(MPath { elements })
                }
            }
        }
    }

    pub fn is_prefix_of_opt<'a, E: IntoIterator<Item = &'a MPathElement>>(
        prefix: Option<&MPath>,
        other: E,
    ) -> bool {
        match prefix {
            Some(prefix) => prefix.is_prefix_of(other),
            None => true,
        }
    }

    pub fn join_opt_element(path: Option<&Self>, element: &MPathElement) -> Self {
        match path {
            Some(path) => path.join_element(Some(element)),
            None => MPath {
                elements: vec![element.clone()],
            },
        }
    }

    pub fn join_element_opt(path: Option<&Self>, element: Option<&MPathElement>) -> Option<Self> {
        match element {
            Some(element) => Self::join_opt(path, element),
            None => path.cloned(),
        }
    }

    pub fn iter_opt(path: Option<&Self>) -> Iter<MPathElement> {
        match path {
            Some(path) => path.into_iter(),
            None => [].iter(),
        }
    }

    pub fn into_iter_opt(path: Option<Self>) -> ::std::vec::IntoIter<MPathElement> {
        match path {
            Some(path) => path.into_iter(),
            None => (vec![]).into_iter(),
        }
    }

    /// The number of components in this path.
    pub fn num_components(&self) -> usize {
        self.elements.len()
    }

    /// The number of leading components that are common.
    pub fn common_components<'a, E: IntoIterator<Item = &'a MPathElement>>(
        &self,
        other: E,
    ) -> usize {
        self.elements
            .iter()
            .zip(other)
            .take_while(|&(e1, e2)| e1 == e2)
            .count()
    }

    /// Whether this path is a path prefix of the given path.
    /// `foo` is a prefix of `foo/bar`, but not of `foo1`.
    #[inline]
    pub fn is_prefix_of<'a, E: IntoIterator<Item = &'a MPathElement>>(&self, other: E) -> bool {
        self.common_components(other.into_iter()) == self.num_components()
    }

    /// The final component of this path.
    pub fn basename(&self) -> &MPathElement {
        self.elements
            .last()
            .expect("MPaths have at least one component")
    }

    /// Create a new path with the number of leading components specified.
    pub fn take_prefix_components(&self, components: usize) -> Result<Option<MPath>> {
        match components {
            0 => Ok(None),
            x if x > self.num_components() => bail!(
                "taking {} components but path only has {}",
                components,
                self.num_components()
            ),
            _ => Ok(Some(MPath {
                elements: self.elements[..components].to_vec(),
            })),
        }
    }

    /// Create a new path, removing `prefix`. Returns `None` if `prefix` is not a strict
    /// prefix of this path - i.e. having removed `prefix`, there are no elements left.
    /// For the intended use case of stripping a directory prefix from a file path,
    /// this is the correct behaviour, since it should not be possible to have
    /// `self == prefix`.
    pub fn remove_prefix_component<'a, E: IntoIterator<Item = &'a MPathElement>>(
        &self,
        prefix: E,
    ) -> Option<MPath> {
        let mut self_iter = self.elements.iter();
        for elem in prefix {
            if Some(elem) != self_iter.next() {
                return None;
            }
        }
        let elements: Vec<_> = self_iter.cloned().collect();
        if elements.is_empty() {
            None
        } else {
            Some(Self { elements })
        }
    }

    pub fn generate<W: Write>(&self, out: &mut W) -> io::Result<()> {
        out.write_all(&self.to_vec())
    }

    pub fn to_vec(&self) -> Vec<u8> {
        let ret: Vec<_> = self.elements.iter().map(|e| e.0.as_ref()).collect();
        ret.join(&b'/')
    }

    #[allow(clippy::len_without_is_empty)]
    /// The length of this path, including any slashes in it.
    pub fn len(&self) -> usize {
        // n elements means n-1 slashes
        let slashes = self.elements.len() - 1;
        let elem_len: usize = self.elements.iter().map(|elem| elem.len()).sum();
        slashes + elem_len
    }

    // Private because it does not validate elements - you must ensure that it's non-empty
    fn from_elements<'a, I>(elements: I) -> Self
    where
        I: Iterator<Item = &'a MPathElement>,
    {
        Self {
            elements: elements.cloned().collect(),
        }
    }

    /// Split an MPath into dirname (if possible) and file name
    pub fn split_dirname(&self) -> (Option<MPath>, &MPathElement) {
        let (filename, dirname_elements) = self
            .elements
            .split_last()
            .expect("MPaths should never be empty");

        if dirname_elements.is_empty() {
            (None, filename)
        } else {
            (
                Some(MPath::from_elements(dirname_elements.iter())),
                filename,
            )
        }
    }

    /// Split an MPath into first path component and the rest
    pub fn split_first(&self) -> (&MPathElement, Option<MPath>) {
        let (first, file_elements) = self
            .elements
            .split_first()
            .expect("MPaths should never be empty");

        if file_elements.is_empty() {
            (first, None)
        } else {
            (first, Some(MPath::from_elements(file_elements.iter())))
        }
    }

    pub fn into_thrift(self) -> thrift::MPath {
        thrift::MPath(
            self.elements
                .into_iter()
                .map(|elem| elem.into_thrift())
                .collect(),
        )
    }

    pub fn display_opt<'a>(path_opt: Option<&'a MPath>) -> DisplayOpt<'a> {
        DisplayOpt(path_opt)
    }

    pub fn get_path_hash(&self) -> MPathHash {
        let mut context = MPathHashContext::new();
        let num_el = self.elements.len();
        if num_el > 0 {
            if num_el > 1 {
                for e in &self.elements[..num_el - 1] {
                    context.update(e.as_ref());
                    context.update([b'/'])
                }
            }
            context.update(self.elements[num_el - 1].as_ref());
        } else {
            context.update([])
        }
        context.finish()
    }

    /// Get an iterator over the parent directories of this `MPath`
    /// Note: it contains the `self` as the first element
    pub fn into_parent_dir_iter(self) -> ParentDirIterator {
        ParentDirIterator {
            current: Some(self),
        }
    }

    pub fn matches_regex(&self, re: &Regex) -> bool {
        let s: String = format!("{}", self);
        re.is_match(&s)
    }
}

pub fn path_bytes_from_mpath(path: Option<&MPath>) -> Vec<u8> {
    match path {
        Some(path) => path.to_vec(),
        None => vec![],
    }
}

impl AsRef<[MPathElement]> for MPath {
    fn as_ref(&self) -> &[MPathElement] {
        &self.elements
    }
}

pub fn mpath_element_iter<'a>(
    mpath: &'a Option<MPath>,
) -> Box<dyn Iterator<Item = &MPathElement> + 'a> {
    match mpath {
        Some(ref path) => Box::new(path.into_iter()),
        None => Box::new(std::iter::empty()),
    }
}

/// Iterator over parent directories of a given `MPath`
pub struct ParentDirIterator {
    current: Option<MPath>,
}

impl Iterator for ParentDirIterator {
    type Item = MPath;

    fn next(&mut self) -> Option<Self::Item> {
        let maybe_current = self.current.take();
        match maybe_current {
            None => None,
            Some(current) => {
                let (maybe_dirname, _) = current.split_dirname();
                self.current = maybe_dirname;
                Some(current)
            }
        }
    }
}

/// Hash of the file path (used in unode)
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub struct MPathHash(Blake2);

impl MPathHash {
    pub fn from_thrift(thrift_path: thrift::MPathHash) -> Result<MPathHash> {
        match thrift_path.0 {
            thrift::IdType::Blake2(blake2) => Ok(MPathHash(Blake2::from_thrift(blake2)?)),
            thrift::IdType::UnknownField(x) => bail!(ErrorKind::InvalidThrift(
                "MPathHash".into(),
                format!("unknown id type field: {}", x)
            )),
        }
    }

    pub fn into_thrift(self) -> thrift::MPathHash {
        thrift::MPathHash(thrift::IdType::Blake2(self.0.into_thrift()))
    }

    pub fn to_hex(&self) -> AsciiString {
        self.0.to_hex()
    }

    pub fn sampling_fingerprint(&self) -> u64 {
        self.0.sampling_fingerprint()
    }
}

/// Context for incrementally computing a hash.
#[derive(Clone)]
pub struct MPathHashContext(Context);

impl MPathHashContext {
    /// Construct a context.
    #[inline]
    pub fn new() -> Self {
        Self(Context::new("mpathhash".as_bytes()))
    }

    #[inline]
    pub fn update<T>(&mut self, data: T)
    where
        T: AsRef<[u8]>,
    {
        self.0.update(data)
    }

    #[inline]
    pub fn finish(self) -> MPathHash {
        MPathHash(self.0.finish())
    }
}

pub struct DisplayOpt<'a>(Option<&'a MPath>);

impl<'a> Display for DisplayOpt<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            Some(path) => write!(f, "{}", path),
            None => write!(f, "(none)"),
        }
    }
}

/// Check that a sorted list of (MPath, is_changed) pairs is path-conflict-free. This means that
/// no changed path in the list (is_changed is true) is a directory of another path.
pub fn check_pcf<'a, I>(sorted_paths: I) -> Result<()>
where
    I: IntoIterator<Item = (&'a MPath, bool)>,
{
    let mut last_changed_path: Option<&MPath> = None;
    // The key observation to make here is that in a sorted list, "foo" will always appear before
    // "foo/bar", which in turn will always appear before "foo1".
    // The loop invariant is that last_changed_path at any point has no prefixes in the list.
    for (path, is_changed) in sorted_paths {
        if let Some(last_changed_path) = last_changed_path {
            if last_changed_path.is_prefix_of(path) {
                bail!(ErrorKind::NotPathConflictFree(
                    last_changed_path.clone(),
                    path.clone(),
                ));
            }
        }
        if is_changed {
            last_changed_path = Some(path);
        }
    }

    Ok(())
}

impl IntoIterator for MPath {
    type Item = MPathElement;
    type IntoIter = ::std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.elements.into_iter()
    }
}

impl<'a> IntoIterator for &'a MPath {
    type Item = &'a MPathElement;
    type IntoIter = Iter<'a, MPathElement>;

    fn into_iter(self) -> Self::IntoIter {
        self.elements.iter()
    }
}

impl<'a> IntoIterator for &'a MPathElement {
    type Item = &'a MPathElement;
    type IntoIter = Once<&'a MPathElement>;

    fn into_iter(self) -> Self::IntoIter {
        once(self)
    }
}

impl<'a> From<&'a MPath> for Vec<u8> {
    fn from(path: &MPath) -> Self {
        path.to_vec()
    }
}

impl<'a> TryFrom<&'a [u8]> for MPath {
    type Error = Error;

    fn try_from(value: &[u8]) -> Result<Self> {
        MPath::new(value)
    }
}

impl<'a> TryFrom<&'a str> for MPath {
    type Error = Error;

    fn try_from(value: &str) -> Result<Self> {
        MPath::new(value.as_bytes())
    }
}

impl TryFrom<Vec<MPathElement>> for MPath {
    type Error = Error;

    fn try_from(elements: Vec<MPathElement>) -> Result<Self> {
        if elements.is_empty() {
            bail!("mpath can not be empty");
        }
        Ok(MPath { elements })
    }
}

lazy_static! {
    static ref COMPONENT_CHARS: Vec<u8> = (2..b'\n')
        .chain((b'\n' + 1)..b'/')
        .chain((b'/' + 1)..255)
        .collect();
}

impl Arbitrary for MPathElement {
    fn arbitrary(g: &mut Gen) -> Self {
        let size = cmp::max(g.size(), 1);
        let size = cmp::min(size, MPATH_ELEMENT_MAX_LENGTH);
        let mut element = SmallVec::with_capacity(size);
        // Keep building possible MPathElements until we get a valid one
        while MPathElement::verify(&element).is_err() {
            element.clear();
            for _ in 0..size {
                let c = g.choose(&COMPONENT_CHARS[..]).unwrap();
                element.push(*c);
            }
        }
        MPathElement(element)
    }
}

impl Arbitrary for MPath {
    #[inline]
    fn arbitrary(g: &mut Gen) -> Self {
        let size = g.size();
        // Up to size components
        //
        // TODO: do we really want a uniform distribution over component chars
        // here?
        let mut path = Vec::new();

        for i in 0..size {
            if i > 0 {
                path.push(b'/');
            }
            let element = MPathElement::arbitrary(g);
            path.extend(&element.0);
        }

        MPath::new(path).unwrap()
    }
}

impl Display for MPathElement {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "{}", String::from_utf8_lossy(&self.0))
    }
}

impl Display for MPath {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "{}", String::from_utf8_lossy(&self.to_vec()))
    }
}

// Implement our own Debug so that strings are displayed properly
impl fmt::Debug for MPathElement {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(
            fmt,
            "MPathElement(\"{}\")",
            String::from_utf8_lossy(&self.0)
        )
    }
}

impl fmt::Debug for MPath {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "MPath(\"{}\")", self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrefixTrie {
    Included,
    Children(HashMap<MPathElement, PrefixTrie>),
}

impl PrefixTrie {
    /// Create a new, empty, prefix trie.
    pub fn new() -> PrefixTrie {
        PrefixTrie::Children(HashMap::new())
    }

    /// Add a path prefix to the prefix trie.  Returns true if the prefix
    /// wasn't already present.
    pub fn add<'p, P: IntoIterator<Item = &'p MPathElement>>(&mut self, path: P) -> bool {
        match self {
            PrefixTrie::Included => false,
            PrefixTrie::Children(children) => {
                let mut iter = path.into_iter();
                match iter.next() {
                    None => {
                        *self = PrefixTrie::Included;
                        true
                    }
                    Some(element) => {
                        if let Some(child) = children.get_mut(element) {
                            return child.add(iter);
                        }
                        children
                            .entry(element.clone())
                            .or_insert_with(PrefixTrie::new)
                            .add(iter)
                    }
                }
            }
        }
    }

    /// Returns true if any path prefix of the given path has previously been
    /// added to the prefix trie.
    pub fn contains_prefix<'p, P: IntoIterator<Item = &'p MPathElement>>(&self, path: P) -> bool {
        match self {
            PrefixTrie::Included => true,
            PrefixTrie::Children(children) => {
                let mut iter = path.into_iter();
                match iter.next() {
                    None => false,
                    Some(element) => {
                        if let Some(child) = children.get(element) {
                            return child.contains_prefix(iter);
                        }
                        false
                    }
                }
            }
        }
    }

    /// Returns true if this trie contains all paths.
    pub fn contains_everything(&self) -> bool {
        self == &PrefixTrie::Included
    }
}

impl Default for PrefixTrie {
    fn default() -> PrefixTrie {
        PrefixTrie::Children(HashMap::new())
    }
}

impl Extend<Option<MPath>> for PrefixTrie {
    fn extend<T: IntoIterator<Item = Option<MPath>>>(&mut self, iter: T) {
        for path in iter {
            if let Some(path) = path {
                self.add(&path);
            } else {
                // The empty path means all paths are included.
                *self = PrefixTrie::Included;
            }
        }
    }
}

impl FromIterator<Option<MPath>> for PrefixTrie {
    fn from_iter<I: IntoIterator<Item = Option<MPath>>>(iter: I) -> Self {
        let mut trie = PrefixTrie::new();
        trie.extend(iter);
        trie
    }
}

pub struct CaseConflictTrie {
    children: HashMap<MPathElement, CaseConflictTrie>,
    lowercase_to_original: HashMap<String, MPathElement>,
}

impl CaseConflictTrie {
    fn new() -> CaseConflictTrie {
        CaseConflictTrie {
            children: HashMap::new(),
            lowercase_to_original: HashMap::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.children.is_empty()
    }

    /// Returns `true` if element was added successfully, or `false`
    /// if trie already contains case conflicting entry.
    fn add<'p, P: IntoIterator<Item = &'p MPathElement>>(
        &mut self,
        path: P,
    ) -> Result<(), ReverseMPath> {
        let mut iter = path.into_iter();
        match iter.next() {
            None => Ok(()),
            Some(element) => {
                if let Some(child) = self.children.get_mut(element) {
                    return child.add(iter).map_err(|mut e| {
                        e.elements.push(element.clone());
                        e
                    });
                }

                if let Some(lower) = element.to_lowercase_utf8() {
                    if let Some(conflict) = self.lowercase_to_original.get(&lower) {
                        return Err(ReverseMPath {
                            elements: vec![conflict.clone()],
                        });
                    } else {
                        self.lowercase_to_original.insert(lower, element.clone());
                    }
                }

                self.children
                    .entry(element.clone())
                    .or_insert_with(CaseConflictTrie::new)
                    .add(iter)
            }
        }
    }

    /// Remove path from a trie
    ///
    /// Returns `true` if path was removed, otherwise `false`.
    fn remove<'p, P: IntoIterator<Item = &'p MPathElement>>(&mut self, path: P) -> bool {
        let mut iter = path.into_iter();
        match iter.next() {
            None => true,
            Some(element) => {
                let (found, remove) = match self.children.get_mut(element) {
                    None => return false,
                    Some(child) => (child.remove(iter), child.is_empty()),
                };
                if remove {
                    self.children.remove(element);

                    if let Some(lower) = element.to_lowercase_utf8() {
                        self.lowercase_to_original.remove(&lower);
                    }
                }
                found
            }
        }
    }
}

struct ReverseMPath {
    /// Elements that are found to conflict. This is in reverse order.
    elements: Vec<MPathElement>,
}

impl ReverseMPath {
    pub fn into_mpath(self) -> MPath {
        let Self { mut elements } = self;
        elements.reverse();
        MPath { elements }
    }
}

pub trait CaseConflictTrieUpdate {
    /// Add this to the CaseConflictTrie. If this results in a case conflict, report the two paths
    /// that conflicted, in the order in which they were added to the CaseConflictTrie.
    fn apply(self, trie: &mut CaseConflictTrie) -> Option<(MPath, MPath)>;
}

impl<'a> CaseConflictTrieUpdate for &'a MPath {
    fn apply(self, trie: &mut CaseConflictTrie) -> Option<(MPath, MPath)> {
        match trie.add(self) {
            Ok(()) => None,
            Err(conflict) => Some((conflict.into_mpath(), self.clone())),
        }
    }
}

impl CaseConflictTrieUpdate for MPath {
    fn apply(self, trie: &mut CaseConflictTrie) -> Option<(MPath, MPath)> {
        match trie.add(&self) {
            Ok(()) => None,
            Err(conflict) => Some((conflict.into_mpath(), self)),
        }
    }
}

impl<'a> CaseConflictTrieUpdate for &'a BonsaiChangeset {
    fn apply(self, trie: &mut CaseConflictTrie) -> Option<(MPath, MPath)> {
        // we need apply deletion first
        for (path, change) in self.file_changes() {
            if change.is_removed() {
                trie.remove(path);
            }
        }
        for (path, change) in self.file_changes() {
            if change.is_changed() {
                if let Some(conflict) = path.apply(trie) {
                    return Some(conflict);
                }
            }
        }
        None
    }
}

/// Returns first path pair that would introduce a case-conflict, if any. The first element is the
/// first one that was added into the Trie, and the second is the last.
pub fn check_case_conflicts<P, I>(iter: I) -> Option<(MPath, MPath)>
where
    P: CaseConflictTrieUpdate,
    I: IntoIterator<Item = P>,
{
    let mut trie = CaseConflictTrie::new();
    for update in iter {
        let conflict = update.apply(&mut trie);
        if conflict.is_some() {
            return conflict;
        }
    }
    None
}

// TODO: Do we need this? Why?
impl<P> FromIterator<P> for CaseConflictTrie
where
    P: CaseConflictTrieUpdate,
{
    fn from_iter<I: IntoIterator<Item = P>>(iter: I) -> Self {
        let mut trie = CaseConflictTrie::new();
        for update in iter {
            let _ = update.apply(&mut trie);
        }
        trie
    }
}

#[cfg(test)]
mod test {
    use std::mem::size_of;

    use quickcheck::quickcheck;
    use quickcheck::TestResult;

    use super::*;

    #[test]
    fn test_mpath_element_size() {
        // MPathElement size is important as we have a lot of them.
        // Test so we are aware of any change.
        assert_eq!(32, size_of::<MPathElement>());
    }

    #[test]
    fn get_path_hash_multiple_elem() {
        let path = MPath::new("foo/bar/baz").unwrap();
        assert_eq!(
            format!("{}", path.get_path_hash().to_hex()).as_str(),
            "4b2cfeded9f9499ffecfed9cea1a36eab97511b241a74c2c84ab8cff45932d1e"
        );
    }

    #[test]
    fn get_path_hash_single_elem() {
        let path = MPath::new("foo").unwrap();
        assert_eq!(
            format!("{}", path.get_path_hash().to_hex()).as_str(),
            "108cf7fc2bbc482daeab0ad8a9af2703cf041ba22ae728df26e1a33d51a3efb0"
        );
    }

    quickcheck! {
        /// Verify that instances generated by quickcheck are valid.
        fn path_gen(p: MPath) -> bool {
            p.elements
                .iter()
                .map(|elem| MPathElement::verify(elem.as_ref()))
                .all(|res| res.is_ok())
        }

        /// Verify that MPathElement instances generated by quickcheck are valid.
        fn pathelement_gen(p: MPathElement) -> bool {
            MPathElement::verify(p.as_ref()).is_ok()
        }

        fn elements_to_path(elements: Vec<MPathElement>) -> TestResult {
            if elements.is_empty() {
                return TestResult::discard();
            }

            let joined = elements.iter().map(|elem| elem.0.clone())
                .collect::<Vec<_>>()
                .join(&b'/');
            let expected_len = joined.len();
            let path = MPath::new(joined).unwrap();
            TestResult::from_bool(elements == path.elements && path.to_vec().len() == expected_len)
        }

        fn path_len(p: MPath) -> bool {
            p.len() == p.to_vec().len()
        }

        fn repo_path_thrift_roundtrip(p: RepoPath) -> bool {
            let thrift_path = p.clone().into_thrift();
            let p2 = RepoPath::from_thrift(thrift_path)
                .expect("converting a valid Thrift structure should always work");
            p == p2
        }

        fn path_thrift_roundtrip(p: MPath) -> bool {
            let thrift_path = p.clone().into_thrift();
            let p2 = MPath::from_thrift(thrift_path)
                .expect("converting a valid Thrift structure should always work");
            p == p2
        }

        fn pathelement_thrift_roundtrip(p: MPathElement) -> bool {
            let thrift_pathelement = p.clone().into_thrift();
            let p2 = MPathElement::from_thrift(thrift_pathelement)
                .expect("converting a valid Thrift structure should always works");
            p == p2
        }
    }

    #[test]
    fn path_make() {
        let path = MPath::new(b"1234abc");
        assert!(MPath::new(b"1234abc").is_ok());
        assert_eq!(path.unwrap().to_vec().len(), 7);
    }

    #[test]
    fn repo_path_make() {
        let path = MPath::new(b"abc").unwrap();
        assert_eq!(
            RepoPath::dir(path.clone()).unwrap(),
            RepoPath::dir("abc").unwrap()
        );
        assert_ne!(RepoPath::dir(path).unwrap(), RepoPath::file("abc").unwrap());
    }

    #[test]
    fn empty_paths() {
        fn assert_empty(path: &str) {
            MPath::new(path).expect_err(&format!(
                "unexpected OK - path '{}' is logically empty",
                path,
            ));
        }
        assert_empty("");
        assert_empty("/");
        assert_empty("//");
        assert_empty("///");
        assert_empty("////");
    }

    #[test]
    fn parent_dir_iterator() {
        fn path(p: &str) -> MPath {
            MPath::new(p).unwrap()
        }

        fn parent_vec(p: &str) -> Vec<MPath> {
            path(p).into_parent_dir_iter().collect()
        }

        assert_eq!(parent_vec("a"), vec![path("a")]);
        assert_eq!(parent_vec("a/b"), vec![path("a/b"), path("a")]);
        assert_eq!(
            parent_vec("a/b/c"),
            vec![path("a/b/c"), path("a/b"), path("a")]
        );
    }

    #[test]
    fn components() {
        let foo = MPath::new("foo").unwrap();
        let foo_bar1 = MPath::new("foo/bar1").unwrap();
        let foo_bar12 = MPath::new("foo/bar12").unwrap();
        let baz = MPath::new("baz").unwrap();

        assert_eq!(foo.common_components(&foo), 1);
        assert_eq!(foo.common_components(&foo_bar1), 1);
        assert_eq!(foo.common_components(&foo_bar12), 1);
        assert_eq!(foo_bar1.common_components(&foo_bar1), 2);
        assert_eq!(foo.common_components(&baz), 0);
        assert_eq!(foo.common_components(MPath::iter_opt(None)), 0);

        assert_eq!(foo_bar1.take_prefix_components(0).unwrap(), None);
        assert_eq!(foo_bar1.take_prefix_components(1).unwrap(), Some(foo));
        assert_eq!(
            foo_bar1.take_prefix_components(2).unwrap(),
            Some(foo_bar1.clone())
        );
        foo_bar1
            .take_prefix_components(3)
            .expect_err("unexpected OK - too many components");
    }

    #[test]
    fn remove_prefix_component() {
        let foo = MPath::new("foo").unwrap();
        let foo_bar1 = MPath::new("foo/bar1").unwrap();
        let foo_bar12 = MPath::new("foo/bar1/2").unwrap();
        let baz = MPath::new("baz").unwrap();
        let bar1 = MPath::new("bar1").unwrap();
        let bar12 = MPath::new("bar1/2").unwrap();
        let two = MPath::new("2").unwrap();

        assert_eq!(baz.remove_prefix_component(&foo), None);
        assert_eq!(foo_bar1.remove_prefix_component(&foo), Some(bar1));
        assert_eq!(foo_bar12.remove_prefix_component(&foo), Some(bar12));
        assert_eq!(foo_bar12.remove_prefix_component(&foo_bar1), Some(two));
    }

    #[test]
    fn bad_path() {
        assert!(MPath::new(b"\0").is_err());
    }
    #[test]
    fn bad_path2() {
        assert!(MPath::new(b"abc\0").is_err());
    }
    #[test]
    fn bad_path3() {
        assert!(MPath::new(b"ab\0cde").is_err());
    }
    #[test]
    fn bad_path4() {
        let p = vec![97; 255];
        assert!(MPath::new(&p).is_ok());

        let p = vec![97; 256];
        assert!(MPath::new(&p).is_err());
    }

    #[test]
    fn bad_path_element() {
        let p = vec![97; 255];
        assert!(MPathElement::new(p).is_ok());

        let p = vec![97; 256];
        assert!(MPathElement::new(p).is_err());
    }

    #[test]
    fn bad_path_thrift() {
        let bad_thrift = thrift::MPath(vec![thrift::MPathElement(b"abc\0".to_vec().into())]);
        MPath::from_thrift(bad_thrift).expect_err("unexpected OK - embedded null");

        let bad_thrift = thrift::MPath(vec![thrift::MPathElement(b"def/ghi".to_vec().into())]);
        MPath::from_thrift(bad_thrift).expect_err("unexpected OK - embedded slash");
    }

    #[test]
    fn path_cmp() {
        let a = MPath::new(b"a").unwrap();
        let b = MPath::new(b"b").unwrap();

        assert!(a < b);
        assert!(a == a);
        assert!(b == b);
        assert!(a <= a);
        assert!(a <= b);
    }

    #[test]
    fn pcf() {
        check_pcf_paths(vec![("foo", true), ("bar", true)])
            .expect("unexpected Err - no directories");
        check_pcf_paths(vec![("foo", true), ("foo/bar", true)])
            .expect_err("unexpected OK - foo is a prefix of foo/bar");
        check_pcf_paths(vec![("foo", false), ("foo/bar", true)])
            .expect("unexpected Err - foo is a prefix of foo/bar but is_changed is false");
        check_pcf_paths(vec![("foo", true), ("foo/bar", false)])
            .expect_err("unexpected OK - foo/bar's is_changed state does not matter");
        check_pcf_paths(vec![("foo", true), ("foo1", true)])
            .expect("unexpected Err - foo is not a path prefix of foo1");
        check_pcf_paths::<_, &str>(vec![])
            .expect("unexpected Err - empty path list has no prefixes");
        // '/' is ASCII 0x2f
        check_pcf_paths(vec![
            ("foo/bar", true),
            ("foo/bar\x2e", true),
            ("foo/bar/baz", true),
            ("foo/bar\x30", true),
        ])
        .expect_err("unexpected OK - other paths and prefixes");
    }

    #[test]
    fn case_conflicts() {
        fn m(mpath: &str) -> MPath {
            MPath::new(mpath).unwrap()
        }

        let mut trie: CaseConflictTrie = vec!["a/b/c", "a/d", "c/d/a"].into_iter().map(m).collect();

        assert!(trie.add(&m("a/b/c")).is_ok());
        assert!(trie.add(&m("a/B/d")).is_err());
        assert!(trie.add(&m("a/b/C")).is_err());
        assert!(trie.remove(&m("a/b/c")));
        assert!(trie.add(&m("a/B/c")).is_ok());

        let paths = vec![
            m("a/b/c"),
            m("a/b/c"), // not a case conflict
            m("a/d"),
            m("a/B/d"),
            m("a/c"),
        ];
        assert_eq!(
            check_case_conflicts(paths.iter()), // works from &MPath
            Some((m("a/b"), m("a/B/d"))),
        );
        assert_eq!(
            check_case_conflicts(paths.into_iter()), // works from MPath
            Some((m("a/b"), m("a/B/d"))),
        );
    }

    fn check_pcf_paths<I, T>(paths: I) -> Result<()>
    where
        I: IntoIterator<Item = (T, bool)>,
        MPath: TryFrom<T, Error = Error>,
    {
        let res: Result<Vec<_>> = paths
            .into_iter()
            .map(|(path, is_changed)| Ok((path.try_into()?, is_changed)))
            .collect();
        let mut paths = res.expect("invalid input path");
        // The input calls for a *sorted* list -- this is important.
        paths.sort_unstable();
        check_pcf(paths.iter().map(|(path, is_changed)| (path, *is_changed)))
    }

    #[test]
    fn prefix_trie() {
        let mut prefixes = PrefixTrie::new();

        let path = |path| MPath::new(path).unwrap();

        // Add some paths
        assert!(prefixes.add(&path("a/b/c")));
        assert!(prefixes.add(&path("a/b/d")));
        assert!(prefixes.add(&path("e")));

        // These paths are already covered by existing prefixes
        assert!(!prefixes.add(&path("a/b/c")));
        assert!(!prefixes.add(&path("e/f")));

        // Expanding a prefix with a more general one is okay
        assert!(prefixes.add(&path("g/h/i")));
        assert!(prefixes.add(&path("g/h")));

        // These paths should match
        assert!(prefixes.contains_prefix(&path("a/b/c/d")));
        assert!(prefixes.contains_prefix(&path("a/b/d/e/f/g")));
        assert!(prefixes.contains_prefix(&path("a/b/d")));
        assert!(prefixes.contains_prefix(&path("e/a")));
        assert!(prefixes.contains_prefix(&path("e/f/g")));
        assert!(prefixes.contains_prefix(&path("e")));
        assert!(prefixes.contains_prefix(&path("g/h")));
        assert!(prefixes.contains_prefix(&path("g/h/i/j")));
        assert!(prefixes.contains_prefix(&path("g/h/k")));

        // These paths should not match
        assert!(!prefixes.contains_prefix(&path("a/b")));
        assert!(!prefixes.contains_prefix(&path("a/b/cc")));
        assert!(!prefixes.contains_prefix(&path("a/b/e")));
        assert!(!prefixes.contains_prefix(&path("a/c")));
        assert!(!prefixes.contains_prefix(&path("a")));
        assert!(!prefixes.contains_prefix(&path("b")));
        assert!(!prefixes.contains_prefix(&path("d")));
        assert!(!prefixes.contains_prefix(&path("abc")));
        assert!(!prefixes.contains_prefix(&path("g")));
        assert!(!prefixes.contains_prefix(&path("g/i")));
        assert!(!prefixes.contains_prefix(&path("g/i/h")));
        assert!(!prefixes.contains_everything());

        // Adding the empty path makes the trie contain everything
        assert!(prefixes.add(&None));
        assert!(prefixes.contains_prefix(&path("a/b/c")));
        assert!(prefixes.contains_prefix(&path("x/y/z")));
        assert!(prefixes.contains_everything());
    }

    #[test]
    fn has_suffix_suffix() {
        let path = |path| MPath::new(path).unwrap();

        // Assert that when the suffix equals the basename the result is
        // correct.
        assert!(&path("a/b/foo.bar").basename().has_suffix(b"foo.bar"));

        // Assert that when the suffix contains is not the basename, the result
        // is correct.
        assert!(!&path("a/b/c").basename().has_suffix(b"b"));

        // Assert when the potential suffix is a suffix the result is correct.
        assert!(&path("a/b/c.bar").basename().has_suffix(b"r"));
        assert!(&path("a/b/c.bar").basename().has_suffix(b"bar"));
        assert!(&path("a/b/c.bar").basename().has_suffix(b".bar"));
        assert!(&path("a/b/c.bar").basename().has_suffix(b"c.bar"));

        // Assert when the potential suffix is not a suffix the result is
        // correct.
        assert!(!&path("a/b/file.bar").basename().has_suffix(b".baz"));
        assert!(!&path("a/b/file.bar").basename().has_suffix(b"baz"));
        assert!(!&path("a/b/c.bar").basename().has_suffix(b"c.baz"));

        // Test case when potential suffix is longer than entire path.
        assert!(
            !&path("a/b/foo.bar")
                .basename()
                .has_suffix(b"file.very_very_very_long_extension")
        );
    }
}
