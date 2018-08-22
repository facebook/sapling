/// Cross-platform local bytes and paths conversion.
///
/// On POSIX, it's a cost-free conversion. No round-trips with UTF-8 strings.
/// On Windows, it's using `MultiByteToWideChar` under the hood.
///
/// Note: The types returned by the functions are different (`Path` vs `PathBuf`)
/// because allocation is needed on Windows.
use std::io;
use std::path::Path;

#[cfg(unix)]
use std::ffi::OsStr;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

/// Convert (usually UTF-8 encoded) `bytes` to `Path`.
///
/// Zero-copy. Cannot return errors. But the Windows version can.
#[cfg(unix)]
#[inline]
pub fn local_bytes_to_path(bytes: &[u8]) -> io::Result<&Path> {
    Ok(Path::new(OsStr::from_bytes(bytes)))
}

/// Convert `Path` to (usually UTF-8 encoded) `bytes`.
///
/// Zero-copy. Cannot return errors. But the Windows version can.
#[cfg(unix)]
#[inline]
pub fn path_to_local_bytes(path: &Path) -> io::Result<&[u8]> {
    Ok(path.as_os_str().as_bytes())
}

// On Windows, use "local_encoding" crate to do the conversion.
// PERF: "local_encoding" API requires round-trip via "String"
// (aka. UTF-8). We might want to bypass that UTF-8 conversion
// so it's [u8] <-> OsString <-> PathBuf.

#[cfg(windows)]
pub extern crate local_encoding;
#[cfg(windows)]
use local_encoding::{Encoder, Encoding};
#[cfg(windows)]
use std::io::ErrorKind::InvalidInput;
#[cfg(windows)]
use std::path::PathBuf;

/// Convert local-encoded `bytes` to `PathBuf`.
///
/// Note: `bytes` are what Mercurial stores in manifests, and are affected
/// by "Language for non-Unicode programs" Windows setting at commit time.
/// Newer APIs might want to normalize paths to UTF-8 before storing them.
#[cfg(windows)]
pub fn local_bytes_to_path(bytes: &[u8]) -> io::Result<PathBuf> {
    Encoding::ANSI.to_string(bytes).map(|s| PathBuf::from(s))
}

/// Convert `Path` to local-encoded `bytes`.
///
/// This is what Mercurial stores. But new programs should probably normalize
/// the path before storing it.
#[cfg(windows)]
pub fn path_to_local_bytes(path: &Path) -> io::Result<Vec<u8>> {
    match path.as_os_str().to_str() {
        Some(s) => Encoding::ANSI.to_bytes(s),
        None => Err(InvalidInput.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Result;

    #[test]
    fn test_ascii7bit_roundtrip() {
        check_roundtrip(b"/var/log/a.log").expect("roundtrip");
    }

    #[test]
    fn test_utf8_roundtrip() {
        let bytes = b"\xE7\xAE\xA1\xE7\x90\x86\xE5\x91\x98\x2F\xE6\xA1\x8C\xE9\x9D\xA2";

        #[cfg(windows)]
        let bytes = {
            match Encoding::OEM.to_bytes(::std::str::from_utf8(bytes).expect("from_utf8")) {
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
}
