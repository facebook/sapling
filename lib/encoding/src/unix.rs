// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::ffi::OsStr;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

#[inline]
pub fn local_bytes_to_osstring(bytes: &[u8]) -> io::Result<&OsStr> {
    Ok(OsStr::from_bytes(bytes))
}

/// Convert `Path` to (usually UTF-8 encoded) `bytes`.
///
/// Zero-copy. Cannot return errors. But the Windows version can.
#[inline]
pub fn path_to_local_bytes(path: &Path) -> io::Result<&[u8]> {
    Ok(path.as_os_str().as_bytes())
}

/// Convert (usually UTF-8 encoded) `bytes` to `Path`.
///
/// Zero-copy. Unix version cannot return errors. Windows version can.
/// Note: `bytes` are what Mercurial stores in manifests, and are affected
/// by "Language for non-Unicode programs" Windows setting at commit time.
/// Newer APIs might want to normalize paths to UTF-8 before storing them.
#[inline]
pub fn local_bytes_to_path(bytes: &[u8]) -> io::Result<&Path> {
    Ok(Path::new(local_bytes_to_osstring(bytes)?))
}

#[inline]
pub fn osstring_to_local_bytes<S: AsRef<OsStr>>(s: &S) -> io::Result<&[u8]> {
    Ok(s.as_ref().as_bytes())
}
