/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use ascii::AsciiString;
use std::cmp;
use std::collections::{HashMap, HashSet};
use std::convert::{From, TryFrom, TryInto};
use std::fmt::{self, Display};
use std::io::{self, Write};
use std::iter::{once, FromIterator, Once};
use std::mem;
use std::slice::Iter;

use abomonation_derive::Abomonation;
use anyhow::{bail, Error, Result};
use asyncmemo::Weight;
use bytes::Bytes;
use failure_ext::chain::ChainExt;
use heapsize::HeapSizeOf;
use heapsize_derive::HeapSizeOf;
use lazy_static::lazy_static;
use quickcheck::{Arbitrary, Gen};
use rand::{seq::SliceRandom, Rng};
use serde_derive::{Deserialize, Serialize};

use crate::bonsai_changeset::BonsaiChangeset;
use crate::errors::ErrorKind;
use crate::hash::{Blake2, Context};
use crate::thrift;

impl Weight for RepoPath {
    fn get_weight(&self) -> usize {
        self.heap_size_of_children() + mem::size_of::<Self>()
    }
}

/// A path or filename within Mononoke, with information about whether
/// it's the root of the repo, a directory or a file.
#[derive(Clone, Debug, PartialEq, Eq, Hash, HeapSizeOf, Serialize, Deserialize)]
pub enum RepoPath {
    // It is now *completely OK* to create a RepoPath directly. All MPaths are valid once
    // constructed.
    RootPath,
    DirectoryPath(MPath),
    FilePath(MPath),
}

// Cacheable instance of RepoPath that can be used inside cachelib
#[derive(Abomonation, Clone, PartialEq, Eq, Hash)]
pub enum RepoPathCached {
    RootPath,
    DirectoryPath(Vec<u8>),
    FilePath(Vec<u8>),
}

impl From<RepoPath> for RepoPathCached {
    fn from(path: RepoPath) -> Self {
        match path {
            RepoPath::RootPath => RepoPathCached::RootPath,
            RepoPath::DirectoryPath(path) => RepoPathCached::DirectoryPath(path.to_vec()),
            RepoPath::FilePath(path) => RepoPathCached::FilePath(path.to_vec()),
        }
    }
}

impl<'a> TryFrom<&'a RepoPathCached> for RepoPath {
    type Error = Error;

    fn try_from(path: &'a RepoPathCached) -> Result<Self> {
        match path {
            RepoPathCached::RootPath => Ok(RepoPath::RootPath),
            RepoPathCached::DirectoryPath(path) => {
                MPath::try_from(path.as_slice()).map(RepoPath::DirectoryPath)
            }
            RepoPathCached::FilePath(path) => {
                MPath::try_from(path.as_slice()).map(RepoPath::FilePath)
            }
        }
    }
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
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct MPathElement(Bytes);

impl MPathElement {
    #[inline]
    pub fn new(element: Vec<u8>) -> Result<MPathElement> {
        Self::verify(&element)?;
        Ok(MPathElement(Bytes::from(element)))
    }

    #[inline]
    pub fn from_thrift(element: thrift::MPathElement) -> Result<MPathElement> {
        Self::verify(&element.0).chain_err(ErrorKind::InvalidThrift(
            "MPathElement".into(),
            "invalid path element".into(),
        ))?;
        Ok(MPathElement(Bytes::from(element.0)))
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
        Ok(())
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn to_bytes(&self) -> Bytes {
        self.0.clone()
    }

    #[inline]
    pub fn into_thrift(self) -> thrift::MPathElement {
        thrift::MPathElement(Vec::from(self.as_ref()))
    }
}

impl HeapSizeOf for MPathElement {
    fn heap_size_of_children(&self) -> usize {
        self.len()
    }
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
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, HeapSizeOf)]
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
        Self::verify(p)?;
        let elements: Vec<_> = p
            .split(|c| *c == b'/')
            .filter(|e| !e.is_empty())
            .map(|e| {
                // These instances have already been checked to contain null bytes and also
                // are split on '/' bytes and non-empty, so they're valid by construction. Skip the
                // verification in MPathElement::new.
                MPathElement(e.into())
            })
            .collect();
        if elements.is_empty() {
            bail!(ErrorKind::InvalidPath(
                String::from_utf8_lossy(p).into_owned(),
                "path cannot be empty".into()
            ));
        }
        Ok(MPath { elements })
    }

    pub fn from_thrift(mpath: thrift::MPath) -> Result<MPath> {
        let elements: Result<Vec<_>> = mpath
            .0
            .into_iter()
            .map(|elem| MPathElement::from_thrift(elem))
            .collect();
        let elements = elements?;

        if elements.is_empty() {
            bail!("Unexpected empty path in thrift::MPath")
        } else {
            Ok(MPath { elements })
        }
    }

    fn verify(p: &[u8]) -> Result<()> {
        if p.contains(&0) {
            bail!(ErrorKind::InvalidPath(
                String::from_utf8_lossy(p).into_owned(),
                "paths cannot contain '\\0'".into(),
            ));
        }
        if p.contains(&1) {
            bail!(ErrorKind::InvalidPath(
                String::from_utf8_lossy(p).into_owned(),
                "paths cannot contain '\\1'".into(),
            ));
        }
        if p.contains(&b'\n') {
            bail!(ErrorKind::InvalidPath(
                String::from_utf8_lossy(p).into_owned(),
                "paths cannot contain '\\n'".into(),
            ));
        }
        Ok(())
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
            None => [].into_iter(),
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
        context.update(self.to_vec());
        context.finish()
    }

    /// Get an iterator over the parent directories of this `MPath`
    /// Note: it contains the `self` as the first element
    pub fn into_parent_dir_iter(self) -> ParentDirIterator {
        ParentDirIterator {
            current: Some(self),
        }
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
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash, HeapSizeOf)]
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
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        let size = cmp::max(g.size(), 1);
        let mut element = Vec::with_capacity(size);
        for _ in 0..size {
            let c = COMPONENT_CHARS[..].choose(g).unwrap();
            element.push(*c);
        }
        MPathElement(Bytes::from(element))
    }
}

impl Arbitrary for MPath {
    #[inline]
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        let size = g.size();
        // Up to sqrt(size) components, each with length from 1 to 2 *
        // sqrt(size) -- don't generate zero-length components. (This isn't
        // verified by MPath::verify() but is good to have as a real distribution
        // of paths.)
        //
        // TODO: deal with or filter out '..' and friends.
        //
        // TODO: do we really want a uniform distribution over component chars
        // here?
        //
        // TODO: this can generate zero-length paths. Consider having separate
        // types for possibly-zero-length and non-zero-length paths.
        let size_sqrt = cmp::max((size as f64).sqrt() as usize, 2);

        let mut path = Vec::new();

        for i in 0..g.gen_range(1, size_sqrt) {
            if i > 0 {
                path.push(b'/');
            }
            path.extend(
                (0..g.gen_range(1, 2 * size_sqrt)).map(|_| *COMPONENT_CHARS[..].choose(g).unwrap()),
            );
        }

        MPath::new(path).unwrap()
    }
}

impl Arbitrary for RepoPath {
    #[inline]
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        match g.next_u32() % 3 {
            0 => RepoPath::RootPath,
            1 => RepoPath::DirectoryPath(MPath::arbitrary(g)),
            2 => RepoPath::FilePath(MPath::arbitrary(g)),
            _ => panic!("Unexpected number in RepoPath::arbitrary"),
        }
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

pub struct CaseConflictTrie {
    children: HashMap<MPathElement, CaseConflictTrie>,
    lowercase: HashSet<String>,
}

impl CaseConflictTrie {
    fn new() -> CaseConflictTrie {
        CaseConflictTrie {
            children: HashMap::new(),
            lowercase: HashSet::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.children.is_empty()
    }

    /// Returns `true` if element was added successfully, or `false`
    /// if trie already contains case conflicting entry.
    fn add<'p, P: IntoIterator<Item = &'p MPathElement>>(&mut self, path: P) -> bool {
        let mut iter = path.into_iter();
        match iter.next() {
            None => true,
            Some(element) => {
                if let Some(child) = self.children.get_mut(&element) {
                    return child.add(iter);
                }

                if let Ok(ref element) = String::from_utf8(Vec::from(element.as_ref())) {
                    let element_lower = element.to_lowercase();
                    if self.lowercase.contains(&element_lower) {
                        return false;
                    } else {
                        self.lowercase.insert(element_lower);
                    }
                }

                self.children
                    .entry(element.clone())
                    .or_insert(CaseConflictTrie::new())
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
                let (found, remove) = match self.children.get_mut(&element) {
                    None => return false,
                    Some(child) => (child.remove(iter), child.is_empty()),
                };
                if remove {
                    self.children.remove(&element);
                    if let Ok(ref element) = String::from_utf8(Vec::from(element.as_ref())) {
                        self.lowercase.remove(&element.to_lowercase());
                    }
                }
                found
            }
        }
    }
}

pub trait CaseConflictTrieUpdate {
    fn apply(self, trie: &mut CaseConflictTrie) -> Option<MPath>;
}

impl<'a> CaseConflictTrieUpdate for &'a MPath {
    fn apply(self, trie: &mut CaseConflictTrie) -> Option<MPath> {
        if !trie.add(self) {
            return Some(self.clone());
        } else {
            None
        }
    }
}

impl CaseConflictTrieUpdate for MPath {
    fn apply(self, trie: &mut CaseConflictTrie) -> Option<MPath> {
        if !trie.add(&self) {
            return Some(self);
        } else {
            None
        }
    }
}

impl<'a> CaseConflictTrieUpdate for &'a BonsaiChangeset {
    fn apply(self, trie: &mut CaseConflictTrie) -> Option<MPath> {
        // we need apply deletion first
        for (path, change) in self.file_changes() {
            if change.is_none() {
                trie.remove(path);
            }
        }
        for (path, change) in self.file_changes() {
            if change.is_some() {
                if !trie.add(path) {
                    return Some(path.clone());
                }
            }
        }
        return None;
    }
}

/// Returns first path that would introduce a case-conflict, if any
pub fn check_case_conflicts<P, I>(iter: I) -> Option<MPath>
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

impl<P> FromIterator<P> for CaseConflictTrie
where
    P: CaseConflictTrieUpdate,
{
    fn from_iter<I: IntoIterator<Item = P>>(iter: I) -> Self {
        let mut trie = CaseConflictTrie::new();
        for update in iter {
            update.apply(&mut trie);
        }
        trie
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use quickcheck::{quickcheck, TestResult};

    quickcheck! {
        /// Verify that instances generated by quickcheck are valid.
        fn path_gen(p: MPath) -> bool {
            let result = MPath::verify(&p.to_vec()).is_ok();
            result && p.elements
                .iter()
                .map(|elem| MPathElement::verify(&elem.as_ref()))
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
                .collect::<Vec<Bytes>>()
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
        assert_eq!(
            foo_bar1.take_prefix_components(1).unwrap(),
            Some(foo.clone())
        );
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
    fn bad_path_thrift() {
        let bad_thrift = thrift::MPath(vec![thrift::MPathElement(b"abc\0".to_vec())]);
        MPath::from_thrift(bad_thrift).expect_err("unexpected OK - embedded null");

        let bad_thrift = thrift::MPath(vec![thrift::MPathElement(b"def/ghi".to_vec())]);
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
        let mut trie: CaseConflictTrie = vec!["a/b/c", "a/d", "c/d/a"]
            .into_iter()
            .map(|p| MPath::new(p).unwrap())
            .collect();

        assert!(trie.add(&MPath::new("a/b/c").unwrap()));
        assert!(trie.add(&MPath::new("a/B/d").unwrap()) == false);
        assert!(trie.add(&MPath::new("a/b/C").unwrap()) == false);
        assert!(trie.remove(&MPath::new("a/b/c").unwrap()));
        assert!(trie.add(&MPath::new("a/B/c").unwrap()));

        let paths = vec![
            MPath::new("a/b/c").unwrap(),
            MPath::new("a/b/c").unwrap(), // not a case conflict
            MPath::new("a/d").unwrap(),
            MPath::new("a/B/d").unwrap(),
            MPath::new("a/c").unwrap(),
        ];
        assert_eq!(
            check_case_conflicts(paths.iter()), // works from &MPath
            Some(MPath::new("a/B/d").unwrap()),
        );
        assert_eq!(
            check_case_conflicts(paths.into_iter()), // works from MPath
            Some(MPath::new("a/B/d").unwrap()),
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
}
