// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use memmap::{Mmap, MmapOptions};
use std::fs::File;
use std::hash::Hasher;
use std::io::{self, Write};
use std::path::Path;
use tempfile;
use twox_hash::{XxHash, XxHash32};

pub use crate::lock::ScopedFileLock;

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
        Some(len) => {
            if len > actual_len {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    format!(
                        "mmap length {} is greater than file size {}",
                        len, actual_len
                    ),
                ));
            } else {
                len
            }
        }
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

/// Open a path. Usually for locking purpose.
///
/// The path is assumed to be a directory. But this function does not do extra
/// checks to make sure. If path is not a directory, this function might still
/// succeed on unix systems.
///
/// Windows does not support opening a directory. This function will create a
/// file called "lock" inside the directory and open that file instead.
pub fn open_dir(lock_path: impl AsRef<Path>) -> io::Result<File> {
    let path = lock_path.as_ref();
    #[cfg(unix)]
    {
        File::open(&path)
    }
    #[cfg(not(unix))]
    {
        use std::fs;
        let mut path = path.to_path_buf();
        path.push("lock");
        File::open(&path).or_else(|_| {
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
        })
    }
}

#[inline]
pub fn xxhash<T: AsRef<[u8]>>(buf: T) -> u64 {
    let mut xx = XxHash::default();
    xx.write(buf.as_ref());
    xx.finish()
}

#[inline]
pub fn xxhash32<T: AsRef<[u8]>>(buf: T) -> u32 {
    let mut xx = XxHash32::default();
    xx.write(buf.as_ref());
    xx.finish() as u32
}

/// Atomically create or replace a file with the given content.
pub fn atomic_write(path: impl AsRef<Path>, content: impl AsRef<[u8]>) -> io::Result<()> {
    let path = path.as_ref();
    let dir = path.parent().expect("path has a parent");
    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    file.as_file_mut().write_all(content.as_ref())?;
    file.persist(path)?;
    Ok(())
}
