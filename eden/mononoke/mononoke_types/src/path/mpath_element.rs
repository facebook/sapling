/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use abomonation_derive::Abomonation;
use anyhow::bail;
use anyhow::Context as _;
use anyhow::Result;
use lazy_static::lazy_static;
use quickcheck::Arbitrary;
use quickcheck::Gen;
use regex::bytes::Regex as BytesRegex;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use smallvec::SmallVec;

use crate::errors::MononokeTypeError;
use crate::thrift;

// Filesystems on Linux commonly limit path *elements* to 255 bytes. Enforce this on MPaths as well
// as a repository that cannot be checked out isn't very useful.
const MPATH_ELEMENT_MAX_LENGTH: usize = 255;

///  Path elements cannot contain any of the following bytes.  '\x01' is not allowed as
/// it is used as a metadata separator.
///
/// This is a mapping from the invalid byte to the representation that should be shown
/// in error messages.
const MPATH_ELEMENT_INVALID_BYTES: &[(u8, &str)] =
    &[(0, "\\0"), (1, "\\x01"), (b'\n', "\\n"), (b'/', "/")];

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
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash)]
#[derive(Abomonation, Serialize, Deserialize)]
pub struct MPathElement(pub(super) SmallVec<[u8; 24]>);

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
    pub fn to_smallvec(self) -> SmallVec<[u8; 24]> {
        self.0
    }

    #[inline]
    pub fn new_from_slice(element: &[u8]) -> Result<MPathElement> {
        Self::verify(element)?;
        Ok(MPathElement(SmallVec::from(element)))
    }

    #[inline]
    pub fn from_thrift(element: thrift::path::MPathElement) -> Result<MPathElement> {
        Self::verify(&element.0).with_context(|| {
            MononokeTypeError::InvalidThrift("MPathElement".into(), "invalid path element".into())
        })?;
        Ok(MPathElement(element.0))
    }

    pub(super) fn verify(p: &[u8]) -> Result<()> {
        if p.is_empty() {
            bail!(MononokeTypeError::InvalidPath(
                "".into(),
                "path elements cannot be empty".into()
            ));
        }
        for (byte, byte_escaped) in MPATH_ELEMENT_INVALID_BYTES {
            if p.contains(byte) {
                bail!(MononokeTypeError::InvalidPath(
                    String::from_utf8_lossy(p).into_owned(),
                    format!("path elements cannot contain '{byte_escaped}'")
                ));
            }
        }
        if p == b"." || p == b".." {
            bail!(MononokeTypeError::InvalidPath(
                String::from_utf8_lossy(p).into_owned(),
                "path elements cannot be . or .. to avoid traversal attacks".into(),
            ));
        }
        Self::check_len(p)?;
        Ok(())
    }

    fn check_len(p: &[u8]) -> Result<()> {
        if p.len() > MPATH_ELEMENT_MAX_LENGTH {
            bail!(MononokeTypeError::InvalidPath(
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
    pub fn into_thrift(self) -> thrift::path::MPathElement {
        thrift::path::MPathElement(self.0)
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

    /// Returns the lowercased version of this NonRootMPath element if it is valid
    /// UTF-8.
    pub fn to_lowercase_utf8(&self) -> Option<String> {
        let s = std::str::from_utf8(self.0.as_ref()).ok()?;
        let s = s.to_lowercase();
        Some(s)
    }

    /// Returns whether this path element is a valid filename on Windows.
    /// ```text
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

lazy_static! {
    /// Regex for looking for invalid windows filenames
    static ref INVALID_WINDOWS_FILENAME_REGEX: BytesRegex =
        BytesRegex::new("^((?i)CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])([.][^.]*|)$")
            .expect("invalid windows filename regex should be valid");
    /// Valid characters for path components
    static ref COMPONENT_CHARS: Vec<u8> = (2..b'\n')
        .chain((b'\n' + 1)..b'/')
        .chain((b'/' + 1)..255)
        .collect();
}

impl AsRef<[u8]> for MPathElement {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl Arbitrary for MPathElement {
    fn arbitrary(g: &mut Gen) -> Self {
        let size = std::cmp::max(g.size(), 1);
        let size = std::cmp::min(size, MPATH_ELEMENT_MAX_LENGTH);
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

impl std::fmt::Display for MPathElement {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(fmt, "{}", String::from_utf8_lossy(&self.0))
    }
}

impl std::fmt::Debug for MPathElement {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            fmt,
            "MPathElement(\"{}\")",
            String::from_utf8_lossy(&self.0)
        )
    }
}

/// A prefix of a path element.
///
/// Restrictions compared to MPathElements are relaxed.  For example, '.' is a
/// valid prefix, as files may begin with '.'.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct MPathElementPrefix(SmallVec<[u8; 24]>);

impl MPathElementPrefix {
    pub fn new() -> Self {
        Self(SmallVec::new())
    }

    pub fn from_slice(s: &[u8]) -> Result<Self> {
        Self::verify(s)?;
        Ok(Self(SmallVec::from_slice(s)))
    }

    /// Append a byte to the end of the prefix.
    pub fn push(&mut self, c: u8) -> Result<()> {
        if self.0.len() == MPATH_ELEMENT_MAX_LENGTH {
            bail!(MononokeTypeError::InvalidPath(
                String::from_utf8_lossy(&self.0).into_owned(),
                format!(
                    "path elements cannot exceed {} bytes",
                    MPATH_ELEMENT_MAX_LENGTH
                )
            ));
        }
        if let Some((_, byte_escaped)) = MPATH_ELEMENT_INVALID_BYTES
            .iter()
            .find(|(byte, _)| byte == &c)
        {
            bail!(MononokeTypeError::InvalidPath(
                String::from_utf8_lossy(&self.0).into_owned(),
                format!("path elements cannot contain '{byte_escaped}'")
            ));
        }
        self.0.push(c);
        Ok(())
    }

    fn verify(p: &[u8]) -> Result<()> {
        for (byte, byte_escaped) in MPATH_ELEMENT_INVALID_BYTES {
            if p.contains(byte) {
                bail!(MononokeTypeError::InvalidPath(
                    String::from_utf8_lossy(p).into_owned(),
                    format!("path elements cannot contain '{byte_escaped}'")
                ));
            }
        }
        MPathElement::check_len(p)?;
        Ok(())
    }

    pub fn to_element(&self) -> Result<MPathElement> {
        MPathElement::new_from_slice(self.0.as_ref())
    }

    pub fn join_into_element(self, suffix: impl AsRef<[u8]>) -> Result<MPathElement> {
        let mut element = self.0;
        element.extend_from_slice(suffix.as_ref());
        MPathElement::from_smallvec(element)
    }
}

impl AsRef<[u8]> for MPathElementPrefix {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl std::fmt::Debug for MPathElementPrefix {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            fmt,
            "MPathElementPrefix(\"{}\")",
            String::from_utf8_lossy(&self.0)
        )
    }
}

#[cfg(test)]
mod tests {
    use std::mem::size_of;

    use quickcheck::quickcheck;

    use super::*;

    #[test]
    fn test_mpath_element_size() {
        // MPathElement size is important as we have a lot of them.
        // Test so we are aware of any change.
        assert_eq!(32, size_of::<MPathElement>());
    }

    quickcheck! {
        /// Verify that MPathElement instances generated by quickcheck are valid.
        fn pathelement_gen(p: MPathElement) -> bool {
            MPathElement::verify(p.as_ref()).is_ok()
        }

        fn pathelement_thrift_roundtrip(p: MPathElement) -> bool {
            let thrift_pathelement = p.clone().into_thrift();
            let p2 = MPathElement::from_thrift(thrift_pathelement)
                .expect("converting a valid Thrift structure should always works");
            p == p2
        }

        fn pathelement_prefixes_all_valid(p: MPathElement, prefix_len: usize) -> bool {
            let prefix_len = prefix_len.min(p.0.len());
            let prefix = &p.0[..prefix_len];
            MPathElementPrefix::verify(prefix).is_ok()
        }

        fn pathelement_prefix_construct_bytewise(p: MPathElement) -> bool {
            let mut prefix = MPathElementPrefix::new();
            for byte in p.0.iter() {
                prefix.push(*byte).expect("prefix should be valid");
            }
            prefix.0 == p.0
        }
    }
}
