// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::cmp;
use std::convert::From;
use std::fmt::{self, Display};
use std::io::{self, Write};
use std::iter::{once, Once};
use std::path::PathBuf;
use std::slice::Iter;
use std::str;

use quickcheck::{Arbitrary, Gen};

use errors::*;

/// A path or filename within Mercurial (typically within manifests or changegroups).
///
/// Mercurial treats pathnames as sequences of bytes, but the manifest format
/// assumes they cannot contain zero bytes. The bytes are not necessarily utf-8
/// and so cannot be converted into a string (or - strictly speaking - be displayed).
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, HeapSizeOf)]
pub struct PathElement(Vec<u8>);

impl PathElement {
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }
}

impl From<PathElement> for Path {
    fn from(element: PathElement) -> Self {
        Path {
            elements: vec![element],
        }
    }
}

#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, HeapSizeOf)]
pub struct Path {
    elements: Vec<PathElement>,
}

impl Path {
    pub fn new<P: AsRef<[u8]>>(p: P) -> Result<Path> {
        let p = p.as_ref();
        Self::verify(p)?;
        let elements: Vec<_> = p.split(|c| *c == b'/')
            .filter(|e| !e.is_empty())
            .map(|e| PathElement(e.into()))
            .collect();
        Ok(Path { elements })
    }

    fn verify(p: &[u8]) -> Result<()> {
        if p.contains(&0) {
            bail!(ErrorKind::InvalidPath("paths cannot contain '\\0'".into()))
        }
        Ok(())
    }

    pub fn join<'a, Elements: IntoIterator<Item = &'a PathElement>>(
        &self,
        another: Elements,
    ) -> Path {
        let mut newelements = self.elements.clone();
        newelements.extend(
            another
                .into_iter()
                .filter(|elem| !elem.0.is_empty())
                .cloned(),
        );
        Path {
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
        Iter: Iterator<Item = &'a PathElement>,
    {
        iter.map(|p| Path::fsencode_filter(direncode(&p.0), dotencode))
            .collect()
    }

    /// Perform the mapping to a filesystem path used in a .hg directory
    /// Assumes that this path is a directory. That means that last path component will also
    /// be mapped as a directory component, not as a file name.
    /// This is necessary if there is file inside .hg that corresponds to the directory in the
    /// repository. For example, this happens in tree manifest. Since directory and files elements
    /// need to be encoded differently, we have two separate methods `fsencode_dir` and
    /// `fsencode_file`. It's up to the user to decide what method to use.
    pub fn fsencode_dir(&self, dotencode: bool) -> PathBuf {
        let prefix = PathElement(Vec::from("meta".as_bytes()));
        let path = ::std::iter::once(&prefix).chain(self.elements.iter());
        Path::fsencode_dir_impl(dotencode, path)
    }

    /// Perform the mapping to a filesystem path used in a .hg directory
    pub fn fsencode_file(&self, dotencode: bool) -> PathBuf {
        // TODO assume fncache
        // TODO doesn't do long path hashing
        let mut path = self.elements.iter().rev();
        let file = path.next();

        let prefix = PathElement(Vec::from("data".as_bytes()));
        let path = ::std::iter::once(&prefix).chain(path.rev());
        let mut ret: PathBuf = Path::fsencode_dir_impl(dotencode, path);

        if let Some(file) = file {
            ret.push(Path::fsencode_filter(&file.0, dotencode));
        }

        ret
    }

    pub fn to_vec(&self) -> Vec<u8> {
        let ret: Vec<_> = self.elements.iter().map(|e| e.0.as_ref()).collect();
        ret.join(&b'/')
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }
}

impl<'a> IntoIterator for &'a Path {
    type Item = &'a PathElement;
    type IntoIter = Iter<'a, PathElement>;

    fn into_iter(self) -> Self::IntoIter {
        self.elements.iter()
    }
}

impl<'a> IntoIterator for &'a PathElement {
    type Item = &'a PathElement;
    type IntoIter = Once<&'a PathElement>;

    fn into_iter(self) -> Self::IntoIter {
        once(self)
    }
}

impl<'a> From<&'a Path> for Vec<u8> {
    fn from(path: &Path) -> Self {
        path.to_vec()
    }
}

lazy_static! {
    static ref COMPONENT_CHARS: Vec<u8> = (1..b'/').chain((b'/' + 1)..255).collect();
}

impl Arbitrary for PathElement {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        let size = cmp::max(g.size(), 1);
        let mut element = Vec::with_capacity(size);
        for _ in 0..size {
            let c = g.choose(&COMPONENT_CHARS[..]).unwrap();
            element.push(*c);
        }
        PathElement(element)
    }
}

impl Arbitrary for Path {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        let size = g.size();
        // Up to sqrt(size) components, each with length from 1 to 2 *
        // sqrt(size) -- don't generate zero-length components. (This isn't
        // verified by Path::verify() but is good to have as a real distribution
        // of paths.)
        //
        // TODO: deal with or filter out '..' and friends.
        //
        // TODO: do we really want a uniform distribution over component chars
        // here?
        let size_sqrt = cmp::max((size as f64).sqrt() as usize, 2);

        let mut path = Vec::new();

        for i in 0..g.gen_range(1, size_sqrt) {
            if i > 0 {
                path.push(b'/');
            }
            path.extend(
                (0..g.gen_range(1, 2 * size_sqrt)).map(|_| g.choose(&COMPONENT_CHARS[..]).unwrap()),
            );
        }

        Path::new(path).unwrap()
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

#[allow(dead_code)] // XXX TODO
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

impl Display for Path {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "{}", String::from_utf8_lossy(&self.to_vec()))
    }
}

// Implement our own Debug so that strings are displayed properly
impl fmt::Debug for PathElement {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "PathElement({:?})", self.0)
    }
}

impl fmt::Debug for Path {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "Path({:?})", self.to_vec())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    quickcheck! {
        /// Verify that instances generated by quickcheck are valid.
        fn path_gen(p: Path) -> bool {
            Path::verify(&p.to_vec()).is_ok()
        }

        fn elements_to_path(elements: Vec<PathElement>) -> bool {
            let joined = elements.iter().map(|elem| elem.0.clone())
                .collect::<Vec<Vec<u8>>>()
                .join(&b'/');
            let expected_len = joined.len();
            let path = Path::new(joined).unwrap();
            elements == path.elements && path.to_vec().len() == expected_len
        }
    }

    #[test]
    fn path_make() {
        let path = Path::new(b"1234abc");
        assert!(Path::new(b"1234abc").is_ok());
        assert_eq!(path.unwrap().to_vec().len(), 7);
    }

    #[test]
    fn bad_path() {
        assert!(Path::new(b"\0").is_err());
    }
    #[test]
    fn bad_path2() {
        assert!(Path::new(b"abc\0").is_err());
    }
    #[test]
    fn bad_path3() {
        assert!(Path::new(b"ab\0cde").is_err());
    }

    #[test]
    fn path_cmp() {
        let a = Path::new(b"a").unwrap();
        let b = Path::new(b"b").unwrap();

        assert!(a < b);
        assert!(a == a);
        assert!(b == b);
        assert!(a <= a);
        assert!(a <= b);
    }

    #[test]
    fn fsencode_simple() {
        let a = Path::new(b"foo/bar").unwrap();
        let p = a.fsencode_file(false);

        assert_eq!(p, PathBuf::from("data/foo/bar"));
    }

    #[test]
    fn fsencode_simple_single() {
        let a = Path::new(b"bar").unwrap();
        let p = a.fsencode_file(false);

        assert_eq!(p, PathBuf::from("data/bar"));
    }

    #[test]
    fn fsencode_hexquote() {
        let a = Path::new(b"oh?/wow~:<>").unwrap();
        let p = a.fsencode_file(false);

        assert_eq!(p, PathBuf::from("data/oh~3f/wow~7e~3a~3c~3e"));
    }

    #[test]
    fn fsencode_direncode() {
        assert_eq!(
            Path::new(b"foo.d/bar.d").unwrap().fsencode_file(false),
            PathBuf::from("data/foo.d.hg/bar.d")
        );
        assert_eq!(
            Path::new(b"foo.d/bar.d").unwrap().fsencode_dir(false),
            PathBuf::from("meta/foo.d.hg/bar.d.hg")
        );
        assert_eq!(
            Path::new(b"foo.hg/bar.d").unwrap().fsencode_file(false),
            PathBuf::from("data/foo.hg.hg/bar.d")
        );
        assert_eq!(
            Path::new(b"foo.hg/bar.d").unwrap().fsencode_dir(false),
            PathBuf::from("meta/foo.hg.hg/bar.d.hg")
        );
        assert_eq!(
            Path::new(b"tests/legacy-encoding.hg")
                .unwrap()
                .fsencode_file(false),
            PathBuf::from("data/tests/legacy-encoding.hg")
        );
        assert_eq!(
            Path::new(b"tests/legacy-encoding.hg")
                .unwrap()
                .fsencode_dir(false),
            PathBuf::from("meta/tests/legacy-encoding.hg.hg")
        );
    }

    #[test]
    fn fsencode_direncode_single() {
        let a = Path::new(b"bar.d").unwrap();
        let p = a.fsencode_file(false);

        assert_eq!(p, PathBuf::from("data/bar.d"));
    }

    #[test]
    fn fsencode_upper() {
        let a = Path::new(b"HELLO/WORLD").unwrap();
        let p = a.fsencode_file(false);

        assert_eq!(p, PathBuf::from("data/_h_e_l_l_o/_w_o_r_l_d"));
    }

    #[test]
    fn fsencode_upper_direncode() {
        let a = Path::new(b"HELLO.d/WORLD.d").unwrap();
        let p = a.fsencode_file(false);

        assert_eq!(p, PathBuf::from("data/_h_e_l_l_o.d.hg/_w_o_r_l_d.d"));
    }

    #[test]
    fn fsencode_single_underscore_fileencode() {
        let a = Path::new(b"_").unwrap();
        let p = a.fsencode_file(false);

        assert_eq!(p, PathBuf::from("data/__"));
    }

    #[test]
    fn fsencode_auxencode() {
        let a = Path::new(b"com3").unwrap();
        let p = a.fsencode_file(false);

        assert_eq!(p, PathBuf::from("data/co~6d3"));

        let a = Path::new(b"lpt9").unwrap();
        let p = a.fsencode_file(false);

        assert_eq!(p, PathBuf::from("data/lp~749"));

        let a = Path::new(b"com").unwrap();
        let p = a.fsencode_file(false);

        assert_eq!(p, PathBuf::from("data/com"));

        let a = Path::new(b"lpt.3").unwrap();
        let p = a.fsencode_file(false);

        assert_eq!(p, PathBuf::from("data/lpt.3"));

        let a = Path::new(b"com3x").unwrap();
        let p = a.fsencode_file(false);

        assert_eq!(p, PathBuf::from("data/com3x"));

        let a = Path::new(b"xcom3").unwrap();
        let p = a.fsencode_file(false);

        assert_eq!(p, PathBuf::from("data/xcom3"));

        let a = Path::new(b"aux").unwrap();
        let p = a.fsencode_file(false);

        assert_eq!(p, PathBuf::from("data/au~78"));

        let a = Path::new(b"auxx").unwrap();
        let p = a.fsencode_file(false);

        assert_eq!(p, PathBuf::from("data/auxx"));

        let a = Path::new(b" ").unwrap();
        let p = a.fsencode_file(true);

        assert_eq!(p, PathBuf::from("data/~20"));

        let a = Path::new(b"aux ").unwrap();
        let p = a.fsencode_file(false);

        assert_eq!(p, PathBuf::from("data/aux~20"));

        let a = Path::new(b"").unwrap();
        let p = a.fsencode_file(false);

        assert_eq!(p, PathBuf::from("data/"));
    }

    #[test]
    fn join() {
        let prefix = Path::new(b"prefix").unwrap();
        assert_eq!(
            prefix
                .join(&Path::new("suffix").unwrap())
                .fsencode_file(false),
            PathBuf::from("data/prefix/suffix")
        );
        assert_eq!(
            prefix.join(&Path::new("").unwrap()).fsencode_file(false),
            PathBuf::from("data/prefix")
        );
        let empty = Path::new(b"").unwrap();
        assert_eq!(
            empty
                .join(&Path::new("suffix").unwrap())
                .fsencode_file(false),
            PathBuf::from("data/suffix")
        );

        assert_eq!(
            Path::new(b"asdf")
                .unwrap()
                .join(&Path::new(b"").unwrap())
                .to_vec()
                .len(),
            4
        );

        assert_eq!(
            Path::new(b"")
                .unwrap()
                .join(&Path::new(b"").unwrap())
                .to_vec()
                .len(),
            0
        );

        assert_eq!(
            Path::new(b"asdf")
                .unwrap()
                .join(&PathElement(b"bdc".iter().cloned().collect()))
                .to_vec()
                .len(),
            8
        );
    }

    #[test]
    fn empty_paths() {
        assert_eq!(Path::new(b"/").unwrap().to_vec().len(), 0);
        assert_eq!(Path::new(b"////").unwrap().to_vec().len(), 0);
        assert_eq!(
            Path::new(b"////")
                .unwrap()
                .join(&Path::new(b"///").unwrap())
                .to_vec()
                .len(),
            0
        );
        let p = b"///";
        let elements: Vec<_> = p.split(|c| *c == b'/')
            .filter(|e| !e.is_empty())
            .map(|e| PathElement(e.into()))
            .collect();
        assert_eq!(
            Path::new(b"////")
                .unwrap()
                .join(elements.iter())
                .to_vec()
                .len(),
            0
        );
        assert!(Path::new(b"////").unwrap().join(elements.iter()).is_empty());
    }
}
