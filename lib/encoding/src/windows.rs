use kernel32;
use local_encoding::{Encoder, Encoding};
use std;
use std::ffi::{OsStr, OsString};
use std::io;
use std::io::ErrorKind::InvalidInput;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use winapi;

const MB_ERR_INVALID_CHARS: winapi::DWORD = 0x00000008;
const WC_COMPOSITECHECK: winapi::DWORD = 0x00000200;

/// Convert local bytes into an `OsString`
/// Since this is a Windows-specific version of this function,
/// it uses the ANSI Code Page (ACP) to perform the conversion
/// from `&[u8]` into `Vec<u16>`, which is then turned into
/// an `OsString`
/// Note that unlike the function in `local_encoding`, this
/// function does not intermediately convert things to
/// `String`, therefore it is "more native" from Windows perspective
/// and is more performant.
#[inline]
pub fn local_bytes_to_osstring(bytes: &[u8]) -> io::Result<OsString> {
    let codepage = winapi::CP_ACP;
    let len = unsafe {
        kernel32::MultiByteToWideChar(
            codepage,
            MB_ERR_INVALID_CHARS,
            bytes.as_ptr() as winapi::LPSTR,
            bytes.len() as i32,
            std::ptr::null_mut(),
            0,
        )
    };
    if len == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut wide: Vec<u16> = Vec::with_capacity(len as usize);
    let len = unsafe {
        wide.set_len(len as usize);
        kernel32::MultiByteToWideChar(
            codepage,
            MB_ERR_INVALID_CHARS,
            bytes.as_ptr() as winapi::LPSTR,
            bytes.len() as i32,
            wide.as_mut_ptr(),
            len,
        )
    };
    if len as usize == wide.len() {
        Ok(OsString::from_wide(&wide))
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Convert `Path` to local-encoded `bytes`.
///
/// This is what Mercurial stores. But new programs should probably normalize
/// the path before storing it.
#[inline]
pub fn path_to_local_bytes(path: &Path) -> io::Result<Vec<u8>> {
    match path.as_os_str().to_str() {
        Some(s) => Encoding::ANSI.to_bytes(s),
        None => Err(InvalidInput.into()),
    }
}

/// Convert (usually UTF-8 encoded) `bytes` to `Path`.
///
/// Zero-copy. Unix version cannot return errors. Windows version can.
/// Note: `bytes` are what Mercurial stores in manifests, and are affected
/// by "Language for non-Unicode programs" Windows setting at commit time.
/// Newer APIs might want to normalize paths to UTF-8 before storing them.
#[inline]
pub fn local_bytes_to_path(bytes: &[u8]) -> io::Result<PathBuf> {
    Ok(PathBuf::from(local_bytes_to_osstring(bytes)?))
}

#[inline]
pub fn osstring_to_local_bytes<S: AsRef<OsStr>>(s: &S) -> io::Result<Vec<u8>> {
    let codepage = winapi::CP_ACP;
    let s: &OsStr = s.as_ref();
    if s.len() == 0 {
        return Ok(Vec::new());
    }
    let wstr: Vec<u16> = s.encode_wide().collect();
    let len = unsafe {
        kernel32::WideCharToMultiByte(
            codepage,
            WC_COMPOSITECHECK,
            wstr.as_ptr(),
            wstr.len() as i32,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
            std::ptr::null_mut(),
        )
    };
    if len == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut astr: Vec<u8> = Vec::with_capacity(len as usize);
    let len = unsafe {
        astr.set_len(len as usize);
        kernel32::WideCharToMultiByte(
            codepage,
            WC_COMPOSITECHECK,
            wstr.as_ptr(),
            wstr.len() as i32,
            astr.as_mut_ptr() as winapi::LPSTR,
            len,
            std::ptr::null(),
            std::ptr::null_mut(),
        )
    };
    if len as usize == astr.len() {
        Ok(astr)
    } else {
        Err(io::Error::last_os_error())
    }
}
