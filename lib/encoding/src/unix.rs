// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::borrow::Cow;
use std::ffi::OsStr;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// Convert bytes in the local encoding to an `OsStr`.
///
/// On Unix, this is a zero-copy operation and cannot fail.  The encoding of the `OsStr` matches
/// that of the original local bytes.
///
/// On Windows, it uses the ANSI Code Page (ACP) to perform the conversion to UTF-16,
/// which is then stored in an `OsString`.  Note that unlike the function in `local_encoding`,
/// this function does not intermediately convert to a Unicode `String`, therefore it is
/// "more native" from Windows' perspective and is more performant.
#[inline]
pub fn local_bytes_to_osstring(bytes: &[u8]) -> io::Result<Cow<'_, OsStr>> {
    Ok(Cow::Borrowed(OsStr::from_bytes(bytes)))
}

/// Convert bytes in the local encoding to a `Path`.
///
/// On Unix, this is a zero-copy operation and cannot fail.
///
/// On Windows, this converts the local bytes to an `OsString` and then converts the
/// `OsString` to a `PathBuf`, possibly returning the same errors as `local_bytes_to_osstring`.
///
/// Note that local bytes are what Mercurial stores in manifests, and are affected
/// by the "Language for non-Unicode programs" Windows setting at commit time.
/// New programs should normalize paths to UTF-8 before storing them.
#[inline]
pub fn local_bytes_to_path(bytes: &[u8]) -> io::Result<Cow<'_, Path>> {
    Ok(Cow::Borrowed(Path::new(OsStr::from_bytes(bytes))))
}

/// Convert an `OsStr` to bytes in the local encoding.
///
/// On Unix, this is a zero-copy operation and cannot fail.  The encoding of the local bytes
/// matches that of the original `OsStr`.
///
/// On Windows, it uses the ANSI Code Page (ACP) to perform the conversion
/// into bytes.  Note that unlike the function in `local_encoding`, this function
/// does not intermediately convert to a Unicode `String`, therefore it is "more native"
/// from Windows' perspective and is more performant.
#[inline]
pub fn osstring_to_local_bytes(s: &OsStr) -> io::Result<Cow<'_, [u8]>> {
    Ok(Cow::Borrowed(s.as_bytes()))
}

/// Convert a `Path` to bytes in the local encoding.
///
/// On Unix, this is a zero-copy operation and cannot fail.
///
/// On Windows, this converts the path to an `OsString` and then converts the
/// `OsString` to local bytes, possibly returning the same errors as
/// `osstring_to_local_bytes`.
///
/// Note that local bytes are what Mercurial stores in manifests, and are affected
/// by the "Language for non-Unicode programs" Windows setting at commit time.
/// New programs should normalize paths to UTF-8 before storing them.
#[inline]
pub fn path_to_local_bytes(path: &Path) -> io::Result<Cow<'_, [u8]>> {
    Ok(Cow::Borrowed(path.as_os_str().as_bytes()))
}
