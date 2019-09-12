// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Cross-platform local bytes and paths conversion.
//!
//! On POSIX, it's a cost-free conversion. No round-trips with UTF-8 strings.
//! On Windows, it's using `MultiByteToWideChar` under the hood.
//!
//! Note: The types returned by the functions are different (`Path` vs `PathBuf`)
//! because allocation is needed on Windows.

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

use std::ffi::{CString, OsStr};
use std::path::Path;

use types::RepoPath;

#[cfg(unix)]
pub use crate::unix::{
    local_bytes_to_osstring, local_bytes_to_path, osstring_to_local_bytes, path_to_local_bytes,
};

#[cfg(windows)]
pub use windows::{
    local_bytes_to_osstring, local_bytes_to_path, osstring_to_local_bytes, path_to_local_bytes,
};

/// Convert a `Path` to a `CString` of local bytes
/// This function panics on failure.
/// On Linux, local bytes are UTF8-encoding of the `Path`
/// On Windows, the `Path` is encoded using current ANSI code page
/// Note that this is *not* a zero-cost function, as `to_vec`
/// copies data. This is needed to bridge the gap between
/// `path_to_local_bytes` return values on different OSes
pub fn path_to_local_cstring(path: &Path) -> CString {
    let bytes: Vec<u8> = path_to_local_bytes(path).unwrap().to_vec();
    unsafe { CString::from_vec_unchecked(bytes) }
}

/// Convert a `&OsStr` to a `CString` of local bytes
/// This function panics on failure
/// On Linux, local bytes are UTF8-encoding of the `&OsStr`
/// On Windows, the encoding is done using the current ANSI code page
/// Note that this is *not* a zero-cost function, as `to_vec`
/// copies data. This is needed to bridge the gap between
/// `osstring_to_local_bytes` return values on different OSes
pub fn osstring_to_local_cstring(os: &OsStr) -> CString {
    let bytes: Vec<u8> = osstring_to_local_bytes(&os).unwrap().to_vec();
    unsafe { CString::from_vec_unchecked(bytes) }
}

/// Converts local bytes `&[u8]` to `&RepoPath`.
/// This function panics on failure.
/// We assume that stored paths are UTF8 encoded and normalized. `RepoPath`
/// represents normalized paths encoded as UTF8. This function is useful
/// because the application has different representations for paths in
/// different contexts. This function marks the crossing of a boundary.
pub fn local_bytes_to_repo_path(bytes: &[u8]) -> &RepoPath {
    RepoPath::from_utf8(bytes).unwrap()
}

/// Converts local bytes `&RepoPath` to `&[u8]`.
/// This function cannot fail.
/// We assume that stored paths are UTF8 encoded and normalized. `RepoPath`
/// represents normalized paths encoded as UTF8. This function is useful
/// because the application has different representations for paths in
/// different contexts. This function marks the crossing of a boundary.
pub fn repo_path_to_local_bytes(path: &RepoPath) -> &[u8] {
    path.as_byte_slice()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::io::Result;

    #[test]
    fn test_ascii7bit_roundtrip() {
        check_roundtrip(b"/var/log/a.log").expect("roundtrip");
        check_roundtrip(b"").expect("roundtrip");
    }

    #[test]
    fn test_utf8_roundtrip() {
        let bytes = b"\xE7\xAE\xA1\xE7\x90\x86\xE5\x91\x98\x2F\xE6\xA1\x8C\xE9\x9D\xA2";

        #[cfg(windows)]
        let bytes = {
            use local_encoding::{Encoder, Encoding};
            match Encoding::ANSI.to_bytes(::std::str::from_utf8(bytes).expect("from_utf8")) {
                Ok(s) => s,
                _ => return, // Cannot be encoded using local encoding. Skip the test.
            }
        };

        check_roundtrip(&bytes[..]).expect("roundtrip");
    }

    fn check_roundtrip(bin_path: &[u8]) -> Result<()> {
        let path = local_bytes_to_path(bin_path)?;
        let bin_path_roundtrip = path_to_local_bytes(&path)?;
        assert_eq!(bin_path[..], bin_path_roundtrip[..]);
        Ok(())
    }

    #[cfg(windows)]
    fn get_encoded_sample() -> (String, Vec<u8>) {
        match unsafe { kernel32::GetACP() } {
            1250 => ("Ł".into(), vec![163]),
            1251 => ("Ї".into(), vec![175]),
            1252 => ("ü".into(), vec![252]),
            _ => ("A".into(), vec![65]),
        }
    }

    #[cfg(unix)]
    fn get_encoded_sample() -> (String, Vec<u8>) {
        ("Їü".into(), vec![208, 135, 195, 188])
    }

    #[test]
    fn test_osstring_to_local_bytes() {
        let (s, b) = get_encoded_sample();
        let os = &OsString::from(s);
        let r = osstring_to_local_bytes(os).unwrap();
        assert_eq!(r[..], b[..]);
    }

    #[test]
    fn test_local_bytes_to_osstring() {
        let (s, b) = get_encoded_sample();
        let r = local_bytes_to_osstring(&b).unwrap();
        assert_eq!(OsString::from(s), r);
    }
}
