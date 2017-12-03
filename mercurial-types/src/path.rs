// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.


use std::cmp;
use std::convert::{From, TryFrom, TryInto};
use std::ffi::OsStr;
use std::fmt::{self, Display};
use std::io::{self, Write};
use std::iter::{once, Once};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::result;
use std::slice::Iter;
use std::str;

use bincode;

use hash::Sha1;
use quickcheck::{Arbitrary, Gen};

use errors::*;

lazy_static! {
    pub static ref DOT: MPathElement = MPathElement(b".".to_vec());
    pub static ref DOTDOT: MPathElement = MPathElement(b"..".to_vec());
}

const MAXSTOREPATHLEN: usize = 120;

/// A path or filename within Mercurial, with information about whether
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
        bincode::serialize(self, bincode::Infinite).expect("serialize for RepoPath cannot fail")
    }

    /// Serialize this RepoPath into a writer. This shouldn't (yet) be considered stable if the
    /// definition of RepoPath changes.
    pub fn serialize_into<W: Write>(&self, writer: &mut W) -> Result<()> {
        Ok(bincode::serialize_into(writer, self, bincode::Infinite)?)
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
    pub fn new(element: Vec<u8>) -> MPathElement {
        MPathElement(element)
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }

    pub fn extend(&mut self, toappend: &[u8]) {
        self.0.extend(toappend.iter());
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

impl From<MPathElement> for MPath {
    fn from(element: MPathElement) -> Self {
        MPath {
            elements: vec![element],
        }
    }
}

/// A path or filename within Mercurial (typically within manifests or changegroups).
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
            .map(|e| MPathElement(e.into()))
            .collect();
        Ok(MPath { elements })
    }

    /// Create a new empty `MPath`.
    pub fn empty() -> Self {
        MPath { elements: vec![] }
    }

    fn verify(p: &[u8]) -> Result<()> {
        if p.contains(&0) {
            bail!(ErrorKind::InvalidPath(
                p.to_vec(),
                "paths cannot contain '\\0'".into()
            ))
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

    pub fn generate<W: Write>(&self, out: &mut W) -> io::Result<()> {
        out.write_all(&self.to_vec())
    }

    fn fsencode_filter<P: AsRef<[u8]>>(p: P, dotencode: bool) -> String {
        let p = p.as_ref();
        let p = fnencode(p);
        let p = auxencode(p, dotencode);
        String::from_utf8(p).expect("bad utf8")
    }

    fn fsencode_dir_impl<'a, Iter>(dotencode: bool, iter: Iter) -> PathBuf
    where
        Iter: Iterator<Item = &'a MPathElement>,
    {
        iter.map(|p| MPath::fsencode_filter(direncode(&p.0), dotencode))
            .collect()
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
}

impl IntoIterator for MPath {
    type Item = MPathElement;
    type IntoIter = ::std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.elements.into_iter()
    }
}

/// Perform the mapping to a filesystem path used in a .hg directory
/// Assumes that this path is a file.
/// This encoding is used when both 'store' and 'fncache' requirements are in the repo.
pub fn fncache_fsencode(elements: &Vec<MPathElement>, dotencode: bool) -> PathBuf {
    let mut path = elements.iter().rev();
    let file = path.next();
    let path = path.rev();
    let mut ret: PathBuf = MPath::fsencode_dir_impl(dotencode, path.clone());

    if let Some(basename) = file {
        ret.push(MPath::fsencode_filter(&basename.0, dotencode));
        let os_str: &OsStr = ret.as_ref();
        if os_str.as_bytes().len() > MAXSTOREPATHLEN {
            hashencode(
                path.map(|elem| elem.0.clone()).collect(),
                &basename.0,
                dotencode,
            )
        } else {
            ret.clone()
        }
    } else {
        PathBuf::new()
    }
}

/// Perform the mapping to a filesystem path used in a .hg directory
/// Assumes that this path is a file.
/// This encoding is used when 'store' requirement is present in the repo, but 'fncache'
/// requirement is not present.
pub fn simple_fsencode(elements: &Vec<MPathElement>) -> PathBuf {
    let mut path = elements.iter().rev();
    let file = path.next();
    let directory_elements = path.rev();

    if let Some(basename) = file {
        let encoded_directory: PathBuf = directory_elements
            .map(|elem| {
                let encoded_element = fnencode(direncode(&elem.0));
                String::from_utf8(encoded_element).expect("bad utf8")
            })
            .collect();

        let encoded_basename =
            PathBuf::from(String::from_utf8(fnencode(&basename.0)).expect("bad utf8"));
        encoded_directory.join(encoded_basename)
    } else {
        PathBuf::new()
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

impl<P: AsRef<[u8]>> TryFrom<P> for MPath {
    type Error = Error;

    fn try_from(value: P) -> Result<Self> {
        MPath::new(value)
    }
}

impl TryFrom<MPath> for MPath {
    type Error = !;

    fn try_from(value: MPath) -> result::Result<Self, !> {
        Ok(value)
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

        for i in 0..g.gen_range(0, size_sqrt) {
            if i > 0 {
                path.push(b'/');
            }
            path.extend(
                (0..g.gen_range(1, 2 * size_sqrt)).map(|_| g.choose(&COMPONENT_CHARS[..]).unwrap()),
            );
        }

        MPath::new(path).unwrap()
    }

    // Skip over shrink for now because it's non-trivial to do.
}

static HEX: &[u8] = b"0123456789abcdef";

fn hexenc(byte: u8, out: &mut Vec<u8>) {
    out.push(b'~');
    out.push(HEX[((byte >> 4) & 0xf) as usize]);
    out.push(HEX[((byte >> 0) & 0xf) as usize]);
}

// Encode directory names
fn direncode(elem: &[u8]) -> Vec<u8> {
    let mut ret = Vec::new();

    ret.extend_from_slice(elem);
    if elem.ends_with(b".hg") || elem.ends_with(b".i") || elem.ends_with(b".d") {
        ret.extend_from_slice(b".hg")
    }

    ret
}

fn fnencode<E: AsRef<[u8]>>(elem: E) -> Vec<u8> {
    let elem = elem.as_ref();
    let mut ret = Vec::new();

    for e in elem {
        let e = *e;
        match e {
            0...31 | 126...255 => hexenc(e, &mut ret),
            b'\\' | b':' | b'*' | b'?' | b'"' | b'<' | b'>' | b'|' => hexenc(e, &mut ret),
            b'A'...b'Z' => {
                ret.push(b'_');
                ret.push(e - b'A' + b'a');
            }
            b'_' => ret.extend_from_slice(b"__"),
            _ => ret.push(e),
        }
    }

    ret
}

fn lowerencode(elem: &[u8]) -> Vec<u8> {
    let mut ret = Vec::new();

    for e in elem {
        let e = *e;
        match e {
            0...31 | 126...255 => hexenc(e, &mut ret),
            b'\\' | b':' | b'*' | b'?' | b'"' | b'<' | b'>' | b'|' => hexenc(e, &mut ret),
            b'A'...b'Z' => {
                ret.push(e - b'A' + b'a');
            }
            _ => ret.push(e),
        }
    }

    ret
}

// if path element is a reserved windows name, remap last character to ~XX
fn auxencode<E: AsRef<[u8]>>(elem: E, dotencode: bool) -> Vec<u8> {
    let elem = elem.as_ref();
    let mut ret = Vec::new();

    if let Some((first, elements)) = elem.split_first() {
        if dotencode && (first == &b'.' || first == &b' ') {
            hexenc(*first, &mut ret);
            ret.extend_from_slice(elements);
        } else {
            // if base portion of name is a windows reserved name,
            // then hex encode 3rd char
            let pos = elem.iter().position(|c| *c == b'.').unwrap_or(elem.len());
            let prefix_len = ::std::cmp::min(3, pos);
            match &elem[..prefix_len] {
                b"aux" | b"con" | b"prn" | b"nul" if pos == 3 => {
                    ret.extend_from_slice(&elem[..2]);
                    hexenc(elem[2], &mut ret);
                    ret.extend_from_slice(&elem[3..]);
                }
                b"com" | b"lpt" if pos == 4 && elem[3] >= b'1' && elem[3] <= b'9' => {
                    ret.extend_from_slice(&elem[..2]);
                    hexenc(elem[2], &mut ret);
                    ret.extend_from_slice(&elem[3..]);
                }
                _ => ret.extend_from_slice(elem),
            }
        }
    }
    // hex encode trailing '.' or ' '
    if let Some(last) = ret.pop() {
        if last == b'.' || last == b' ' {
            hexenc(last, &mut ret);
        } else {
            ret.push(last);
        }
    }

    ret
}

/// Returns file extension with period; leading periods are ignored
///
/// # Examples
/// ```
/// assert_eq(get_extension(b".foo"), b"");
/// assert_eq(get_extension(b"bar.foo"), b".foo");
/// assert_eq(get_extension(b"foo."), b".");
///
/// ```
fn get_extension(basename: &[u8]) -> &[u8] {
    let idx = basename
        .iter()
        .enumerate()
        .rev()
        .find(|&(_, c)| *c == b'.')
        .map(|(idx, _)| idx);
    match idx {
        None | Some(0) => b"",
        Some(idx) => &basename[idx..],
    }
}

/// Returns sha-1 hash of the file name
///
/// # Example
/// ```
/// let dirs = vec![Vec::from(&b"asdf"[..]), Vec::from("asdf")];
/// let file = b"file.txt";
/// assert_eq!(hashed_file(&dirs, Some(file)), Sha1::from(&b"asdf/asdf/file.txt"[..]));
///
/// ```
fn hashed_file(dirs: &Vec<Vec<u8>>, file: &[u8]) -> Sha1 {
    let mut elements: Vec<_> = dirs.iter().map(|elem| direncode(&elem)).collect();
    elements.push(Vec::from(file));

    Sha1::from(elements.join(&b'/').as_ref())
}

/// This function emulates core mercurial _hashencode() function. In core mercurial it is used
/// if path is longer than MAXSTOREPATHLEN.
/// Resulting path starts with "dh/" prefix, it has the same extension as initial file, and it
/// also contains sha-1 hash of the initial file.
/// Each path element is encoded to avoid file name collisions, and they are combined
/// in such way that resulting path is shorter than MAXSTOREPATHLEN.
fn hashencode(dirs: Vec<Vec<u8>>, file: &[u8], dotencode: bool) -> PathBuf {
    let sha1 = hashed_file(&dirs, file);

    let mut parts = dirs.iter()
        .map(|elem| direncode(&elem))
        .map(|elem| lowerencode(&elem))
        .map(|elem| auxencode(elem, dotencode));

    let mut shortdirs = Vec::new();
    // Prefix (which is usually 'data' or 'meta') is replaced with 'dh'.
    // Other directories will be converted.
    let prefix = parts.next();
    let prefix_len = prefix.map(|elem| elem.len()).unwrap_or(0);
    // Each directory is no longer than `dirprefixlen`, and total length is less than
    // `maxshortdirslen`. These constants are copied from core mercurial code.
    let dirprefixlen = 8;
    let maxshortdirslen = 8 * (dirprefixlen + 1) - prefix_len;
    let mut shortdirslen = 0;
    for p in parts {
        let size = cmp::min(dirprefixlen, p.len());
        let dir = &p[..size];
        let dir = match dir.split_last() {
            Some((last, prefix)) => if last == &b'.' || last == &b' ' {
                let mut vec = Vec::from(prefix);
                vec.push(b'_');
                vec
            } else {
                Vec::from(dir)
            },
            _ => Vec::from(dir),
        };

        if shortdirslen == 0 {
            shortdirslen = dir.len();
        } else {
            // 1 is for '/'
            let t = shortdirslen + 1 + dir.len();
            if t > maxshortdirslen {
                break;
            }
            shortdirslen = t;
        }
        shortdirs.push(dir);
    }
    let mut shortdirs = shortdirs.join(&b'/');
    if !shortdirs.is_empty() {
        shortdirs.push(b'/');
    }

    // File name encoding is a bit different from directory element encoding - direncode() is not
    // applied.
    let basename = auxencode(lowerencode(file), dotencode);
    let hex_sha = sha1.to_hex();

    let mut res = vec![];
    res.push(&b"dh/"[..]);
    res.push(&shortdirs);
    // filler is inserted after shortdirs but before sha. Let's remember the index.
    // This is part of the basename that is as long as possible given that the resulting string
    // is shorter that MAXSTOREPATHLEN.
    let filler_index = res.len();
    res.push(hex_sha.as_bytes());
    res.push(get_extension(&basename));

    let filler = {
        let current_len = res.iter().map(|elem| elem.len()).sum::<usize>();
        let spaceleft = MAXSTOREPATHLEN - current_len;
        let size = cmp::min(basename.len(), spaceleft);
        &basename[..size]
    };
    res.insert(filler_index, filler);
    PathBuf::from(String::from_utf8(res.concat()).expect("bad utf8"))
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
        write!(fmt, "MPath({:?})", self.to_vec())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    quickcheck! {
        /// Verify that instances generated by quickcheck are valid.
        fn path_gen(p: MPath) -> bool {
            MPath::verify(&p.to_vec()).is_ok()
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
        assert_matches!(
            RepoPath::file(path),
            Err(Error(ErrorKind::InvalidMPath(_, _), _))
        );

        assert_matches!(
            RepoPath::dir(b""),
            Err(Error(ErrorKind::InvalidMPath(_, _), _))
        );
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
    fn path_cmp() {
        let a = MPath::new(b"a").unwrap();
        let b = MPath::new(b"b").unwrap();

        assert!(a < b);
        assert!(a == a);
        assert!(b == b);
        assert!(a <= a);
        assert!(a <= b);
    }

    fn check_fsencode(path: &[u8], expected: &str) {
        let mut elements = vec![];
        let path = &MPath::new(path).unwrap();
        elements.extend(path.into_iter().cloned());

        assert_eq!(fncache_fsencode(&elements, false), PathBuf::from(expected));
    }

    fn check_simple_fsencode(path: &[u8], expected: &str) {
        let mut elements = vec![];
        let path = &MPath::new(path).unwrap();
        elements.extend(path.into_iter().cloned());

        assert_eq!(simple_fsencode(&elements), PathBuf::from(expected));
    }

    #[test]
    fn fsencode_simple() {
        check_fsencode(b"foo/bar", "foo/bar");
    }

    #[test]
    fn fsencode_simple_single() {
        check_fsencode(b"bar", "bar");
    }

    #[test]
    fn fsencode_hexquote() {
        check_fsencode(b"oh?/wow~:<>", "oh~3f/wow~7e~3a~3c~3e");
    }

    #[test]
    fn fsencode_direncode() {
        check_fsencode(b"foo.d/bar.d", "foo.d.hg/bar.d");
        check_fsencode(b"foo.d/bar.d/file", "foo.d.hg/bar.d.hg/file");
        check_fsencode(b"tests/legacy-encoding.hg", "tests/legacy-encoding.hg");
        check_fsencode(
            b"tests/legacy-encoding.hg/file",
            "tests/legacy-encoding.hg.hg/file",
        );
    }

    #[test]
    fn fsencode_direncode_single() {
        check_fsencode(b"bar.d", "bar.d");
    }

    #[test]
    fn fsencode_upper() {
        check_fsencode(b"HELLO/WORLD", "_h_e_l_l_o/_w_o_r_l_d");
    }

    #[test]
    fn fsencode_upper_direncode() {
        check_fsencode(b"HELLO.d/WORLD.d", "_h_e_l_l_o.d.hg/_w_o_r_l_d.d");
    }

    #[test]
    fn fsencode_single_underscore_fileencode() {
        check_fsencode(b"_", "__");
    }

    #[test]
    fn fsencode_auxencode() {
        check_fsencode(b"com3", "co~6d3");
        check_fsencode(b"lpt9", "lp~749");
        check_fsencode(b"com", "com");
        check_fsencode(b"lpt.3", "lpt.3");
        check_fsencode(b"com3x", "com3x");
        check_fsencode(b"xcom3", "xcom3");
        check_fsencode(b"aux", "au~78");
        check_fsencode(b"auxx", "auxx");
        check_fsencode(b" ", "~20");
        check_fsencode(b"aux ", "aux~20");
    }

    fn join_and_check(prefix: &str, suffix: &str, expected: &str) {
        let prefix = MPath::new(prefix).unwrap();
        let mut elements = vec![];
        let joined = &prefix.join(&MPath::new(suffix).unwrap());
        elements.extend(joined.into_iter().cloned());
        assert_eq!(fncache_fsencode(&elements, false), PathBuf::from(expected));
    }

    #[test]
    fn join() {
        join_and_check("prefix", "suffix", "prefix/suffix");
        join_and_check("prefix", "", "prefix");
        join_and_check("", "suffix", "suffix");

        assert_eq!(
            MPath::new(b"asdf")
                .unwrap()
                .join(&MPath::new(b"").unwrap())
                .to_vec()
                .len(),
            4
        );

        assert_eq!(
            MPath::new(b"")
                .unwrap()
                .join(&MPath::new(b"").unwrap())
                .to_vec()
                .len(),
            0
        );

        assert_eq!(
            MPath::new(b"asdf")
                .unwrap()
                .join(&MPathElement(b"bdc".iter().cloned().collect()))
                .to_vec()
                .len(),
            8
        );
    }

    #[test]
    fn empty_paths() {
        assert_eq!(MPath::new(b"/").unwrap().to_vec().len(), 0);
        assert_eq!(MPath::new(b"////").unwrap().to_vec().len(), 0);
        assert_eq!(
            MPath::new(b"////")
                .unwrap()
                .join(&MPath::new(b"///").unwrap())
                .to_vec()
                .len(),
            0
        );
        let p = b"///";
        let elements: Vec<_> = p.split(|c| *c == b'/')
            .filter(|e| !e.is_empty())
            .map(|e| MPathElement(e.into()))
            .collect();
        assert_eq!(
            MPath::new(b"////")
                .unwrap()
                .join(elements.iter())
                .to_vec()
                .len(),
            0
        );
        assert!(
            MPath::new(b"////")
                .unwrap()
                .join(elements.iter())
                .is_empty()
        );
    }

    #[test]
    fn test_get_extension() {
        assert_eq!(get_extension(b".foo"), b"");
        assert_eq!(get_extension(b"foo."), b".");
        assert_eq!(get_extension(b"foo"), b"");
        assert_eq!(get_extension(b"foo.txt"), b".txt");
        assert_eq!(get_extension(b"foo.bar.blat"), b".blat");
    }

    #[test]
    fn test_hashed_file() {
        let dirs = vec![Vec::from(&b"asdf"[..]), Vec::from("asdf")];
        let file = b"file.txt";
        assert_eq!(
            hashed_file(&dirs, file),
            Sha1::from(&b"asdf/asdf/file.txt"[..])
        );
    }

    #[test]
    fn test_fsencode() {
        let toencode = b"data/abcdefghijklmnopqrstuvwxyz0123456789 !#%&'()+,-.;=[]^`{}";
        let expected = "data/abcdefghijklmnopqrstuvwxyz0123456789 !#%&'()+,-.;=[]^`{}";
        check_fsencode(&toencode[..], expected);

        let toencode = b"data/\x01\x02\x03\x04\x05\x06\x07\x08\t\n\x0b\x0c\r\x0e\x0f\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f";
        let expected = "data/~01~02~03~04~05~06~07~08~09~0a~0b~0c~0d~0e~0f~10~11~12~13~14~15~16~17~18~19~1a~1b~1c~1d~1e~1f";
        check_fsencode(&toencode[..], expected);
    }

    #[test]
    fn test_simple_fsencode() {
        let toencode: &[u8] = b"foo.i/bar.d/bla.hg/hi:world?/HELLO";
        let expected = "foo.i.hg/bar.d.hg/bla.hg.hg/hi~3aworld~3f/_h_e_l_l_o";

        check_simple_fsencode(toencode, expected);

        let toencode: &[u8] = b".arcconfig.i";
        let expected = ".arcconfig.i";
        check_simple_fsencode(toencode, expected);
    }
}
