/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use indexedlog::utils::mmap_bytes;
use minibytes::Bytes;
use std::fs;
use std::io;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::path::Path;

/// Read file at `path`.
/// If the file does not exist, return `fallback`.
/// If `truncatable` is true, avoid using mmap on Windows.
pub(crate) fn read_path(path: &Path, fallback: Bytes, truncatable: bool) -> io::Result<Bytes> {
    match fs::OpenOptions::new().read(true).open(path) {
        Err(err) => {
            if err.kind() == io::ErrorKind::NotFound {
                Ok(fallback)
            } else {
                Err(err)
            }
        }
        Ok(mut file) => {
            if truncatable && cfg!(windows) {
                let size = file.seek(SeekFrom::End(0))? as usize;
                let mut buf = vec![0u8; size];
                file.seek(SeekFrom::Start(0))?;
                file.read_exact(&mut buf)?;
                Ok(buf.into())
            } else {
                mmap_bytes(&file, None)
            }
        }
    }
}
