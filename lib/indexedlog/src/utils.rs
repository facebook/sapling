use memmap::{Mmap, MmapOptions};
use std::fs::File;
use std::hash::Hasher;
use std::io;
use twox_hash::XxHash;

/// Return a read-only mmap view of the entire file, and its length.
///
/// If `len` is `None`, detect the file length automatically.
///
/// For an empty file, return (1-byte mmap, 0) instead.
///
/// The caller might want to use some kind of locking to make
/// sure the file length is at some kind of boundary.
pub fn mmap_readonly(file: &File, len: Option<u64>) -> io::Result<(Mmap, u64)> {
    let actual_len = file.metadata()?.len();
    let len = match len {
        Some(len) => if len > actual_len {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!(
                    "mmap length {} is greater than file size {}",
                    len, actual_len
                ),
            ));
        } else {
            len
        },
        None => actual_len,
    };
    let mmap = unsafe {
        if len == 0 {
            MmapOptions::new().len(1).map_anon()?.make_read_only()?
        } else {
            MmapOptions::new().len(len as usize).map(&file)?
        }
    };
    Ok((mmap, len))
}

#[inline]
pub fn xxhash<T: AsRef<[u8]>>(buf: T) -> u64 {
    let mut xx = XxHash::default();
    xx.write(buf.as_ref());
    xx.finish()
}
