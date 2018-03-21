// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::cmp;
use std::convert::{From, TryFrom, TryInto};
use std::fmt::{self, Display};
use std::io::{self, Write};
use std::iter::{once, Once};
use std::slice::Iter;

use bincode;

use quickcheck::{Arbitrary, Gen};

use errors::*;
use thrift;

lazy_static! {
    pub static ref DOT: MPathElement = MPathElement(b".".to_vec());
    pub static ref DOTDOT: MPathElement = MPathElement(b"..".to_vec());
}

/// A path or filename within Mononoke, with information about whether
/// it's the root of the repo, a directory or a file.
#[derive(Clone, Debug, PartialEq, Eq, Hash, HeapSizeOf)]
#[derive(Serialize, Deserialize)]
pub enum RepoPath {
    // Do not create a RepoPath directly! Go through the constructors instead -- they verify MPath
    // properties.
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
        if path.is_empty() {
            Err(
                ErrorKind::InvalidMPath(path, "RepoPath does not support empty MPaths".into())
                    .into(),
            )
        } else {
            Ok(RepoPath::DirectoryPath(path))
        }
    }

    pub fn file<P>(path: P) -> Result<Self>
    where
        P: TryInto<MPath>,
        Error: From<P::Error>,
    {
        let path = path.try_into()?;
        if path.is_empty() {
            Err(
                ErrorKind::InvalidMPath(path, "RepoPath does not support empty MPaths".into())
                    .into(),
            )
        } else {
            Ok(RepoPath::FilePath(path))
        }
    }

    pub fn mpath(&self) -> Option<&MPath> {
        match *self {
            RepoPath::RootPath => None,
            RepoPath::DirectoryPath(ref path) => Some(path),
            RepoPath::FilePath(ref path) => Some(path),
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
}

impl Display for RepoPath {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            RepoPath::RootPath => write!(f, "root"),
            RepoPath::DirectoryPath(ref path) => write!(f, "directory {}", path),
            RepoPath::FilePath(ref path) => write!(f, "file {}", path),
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
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, HeapSizeOf)]
#[derive(Serialize, Deserialize)]
pub struct MPathElement(Vec<u8>);

impl MPathElement {
    #[inline]
    pub fn new(element: Vec<u8>) -> Result<MPathElement> {
        Self::verify(&element)?;
        Ok(MPathElement(element))
    }

    #[inline]
    pub(crate) fn from_thrift(element: thrift::MPathElement) -> Result<MPathElement> {
        Self::verify(&element.0).context(ErrorKind::InvalidThrift(
            "MPathElement".into(),
            "invalid path element".into(),
        ))?;
        Ok(MPathElement(element.0))
    }

    fn verify(p: &[u8]) -> Result<()> {
        if p.is_empty() {
            bail_err!(ErrorKind::InvalidPath(
                "".into(),
                "path elements cannot be empty".into()
            ));
        }
        if p.contains(&0) {
            bail_err!(ErrorKind::InvalidPath(
                String::from_utf8_lossy(p).into_owned(),
                "path elements cannot contain '\\0'".into(),
            ));
        }
        if p.contains(&b'/') {
            bail_err!(ErrorKind::InvalidPath(
                String::from_utf8_lossy(p).into_owned(),
                "path elements cannot contain '/'".into(),
            ));
        }
        Ok(())
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }

    #[inline]
    pub fn to_bytes(&self) -> Vec<u8> {
        self.0.clone()
    }

    pub fn extend(&mut self, toappend: &[u8]) {
        self.0.extend(toappend.iter());
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub(crate) fn into_thrift(self) -> thrift::MPathElement {
        thrift::MPathElement(self.0)
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

impl MPath {
    pub fn new<P: AsRef<[u8]>>(p: P) -> Result<MPath> {
        let p = p.as_ref();
        Self::verify(p)?;
        let elements: Vec<_> = p.split(|c| *c == b'/')
            .filter(|e| !e.is_empty())
            .map(|e| {
                // These instances have already been checked to contain null bytes and also
                // are split on '/' bytes and non-empty, so they're valid by construction. Skip the
                // verification in MPathElement::new.
                MPathElement(e.into())
            })
            .collect();
        Ok(MPath { elements })
    }

    /// Create a new empty `MPath`.
    pub fn empty() -> Self {
        MPath { elements: vec![] }
    }

    pub(crate) fn from_thrift(mpath: thrift::MPath) -> Result<MPath> {
        let elements: Result<Vec<_>> = mpath
            .0
            .into_iter()
            .map(|elem| MPathElement::from_thrift(elem))
            .collect();
        Ok(MPath {
            elements: elements?,
        })
    }

    fn verify(p: &[u8]) -> Result<()> {
        if p.contains(&0) {
            bail_err!(ErrorKind::InvalidPath(
                String::from_utf8_lossy(p).into_owned(),
                "paths cannot contain '\\0'".into(),
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

    pub fn generate<W: Write>(&self, out: &mut W) -> io::Result<()> {
        out.write_all(&self.to_vec())
    }

    pub fn to_vec(&self) -> Vec<u8> {
        let ret: Vec<_> = self.elements.iter().map(|e| e.0.as_ref()).collect();
        ret.join(&b'/')
    }

    /// The length of this path, including any slashes in it.
    pub fn len(&self) -> usize {
        if self.is_empty() {
            0
        } else {
            // n elements means n-1 slashes
            let slashes = self.elements.len() - 1;
            let elem_len: usize = self.elements.iter().map(|elem| elem.len()).sum();
            slashes + elem_len
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    pub(crate) fn into_thrift(self) -> thrift::MPath {
        thrift::MPath(
            self.elements
                .into_iter()
                .map(|elem| elem.into_thrift())
                .collect(),
        )
    }
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

lazy_static! {
    static ref COMPONENT_CHARS: Vec<u8> = (1..b'/').chain((b'/' + 1)..255).collect();
}

impl Arbitrary for MPathElement {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        let size = cmp::max(g.size(), 1);
        let mut element = Vec::with_capacity(size);
        for _ in 0..size {
            let c = g.choose(&COMPONENT_CHARS[..]).unwrap();
            element.push(*c);
        }
        MPathElement(element)
    }
}

impl Arbitrary for MPath {
    #[inline]
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        // Note that this can generate zero-length paths. To only generate non-zero-length paths,
        // use arbitrary_params.
        Self::arbitrary_params(g, true)
    }

    // Skip over shrink for now because it's non-trivial to do.
}

impl MPath {
    pub fn arbitrary_params<G: Gen>(g: &mut G, empty_allowed: bool) -> Self {
        let min_components = if empty_allowed { 0 } else { 1 };
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

        for i in 0..g.gen_range(min_components, size_sqrt) {
            if i > 0 {
                path.push(b'/');
            }
            path.extend(
                (0..g.gen_range(1, 2 * size_sqrt)).map(|_| g.choose(&COMPONENT_CHARS[..]).unwrap()),
            );
        }

        MPath::new(path).unwrap()
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
            "MPathElement({:?} \"{}\")",
            self.0,
            String::from_utf8_lossy(&self.0)
        )
    }
}

impl fmt::Debug for MPath {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "MPath({:?} \"{}\")", self.to_vec(), self)
    }
}

#[cfg(test)]
mod test {
    use quickcheck::StdGen;
    use rand;

    use super::*;

    quickcheck! {
        /// Verify that instances generated by quickcheck are valid.
        fn path_gen(p: MPath) -> bool {
            let result = MPath::verify(&p.to_vec()).is_ok();
            result && p.elements
                .iter()
                .map(|elem| MPathElement::verify(&elem.as_bytes()))
                .all(|res| res.is_ok())
        }

        /// Verify that MPathElement instances generated by quickcheck are valid.
        fn pathelement_gen(p: MPathElement) -> bool {
            MPathElement::verify(p.as_bytes()).is_ok()
        }

        fn elements_to_path(elements: Vec<MPathElement>) -> bool {
            let joined = elements.iter().map(|elem| elem.0.clone())
                .collect::<Vec<Vec<u8>>>()
                .join(&b'/');
            let expected_len = joined.len();
            let path = MPath::new(joined).unwrap();
            elements == path.elements && path.to_vec().len() == expected_len
        }

        fn path_len(p: MPath) -> bool {
            p.len() == p.to_vec().len()
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

    /// Verify that arbitrary instances with empty_allowed set to false are not empty.
    #[test]
    fn path_non_empty() {
        let mut rng = StdGen::new(rand::thread_rng(), 100);
        for _n in 0..100 {
            let path = MPath::arbitrary_params(&mut rng, false);
            MPath::verify(&path.to_vec()).expect("arbitrary MPath should be valid");
            assert!(
                !path.is_empty(),
                "empty_allowed is false so empty paths should not be generated"
            );
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
    fn repo_path_empty() {
        let path = MPath::new("").unwrap();
        match RepoPath::file(path) {
            Ok(bad) => panic!("unexpected success {:?}", bad),
            Err(err) => assert_matches!(
                err.downcast::<ErrorKind>().unwrap(),
                ErrorKind::InvalidMPath(_, _)
            ),
        };

        match RepoPath::dir("") {
            Ok(bad) => panic!("unexpected success {:?}", bad),
            Err(err) => assert_matches!(
                err.downcast::<ErrorKind>().unwrap(),
                ErrorKind::InvalidMPath(_, _)
            ),
        };
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
}
