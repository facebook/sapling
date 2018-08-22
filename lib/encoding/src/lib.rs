/// Cross-platform local bytes and paths conversion.
///
/// On POSIX, it's a cost-free conversion. No round-trips with UTF-8 strings.
/// On Windows, it's using `MultiByteToWideChar` under the hood.
///
/// Note: The types returned by the functions are different (`Path` vs `PathBuf`)
/// because allocation is needed on Windows.
#[cfg(windows)]
extern crate kernel32;
#[cfg(windows)]
extern crate local_encoding;
#[cfg(windows)]
extern crate winapi;

#[cfg(windows)]
mod windows;
#[cfg(unix)]
mod unix;

#[cfg(unix)]
pub use unix::{
    local_bytes_to_osstring,
    local_bytes_to_path,
    path_to_local_bytes
};

#[cfg(windows)]
pub use windows::{
    local_bytes_to_osstring,
    local_bytes_to_path,
    path_to_local_bytes
};

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
}
