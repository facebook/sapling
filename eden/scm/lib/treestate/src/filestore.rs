/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Implementation of a store using file I/O.

use crate::errors::ErrorKind;
use crate::store::{BlockId, Store, StoreView};
use byteorder::{BigEndian, ByteOrder, ReadBytesExt, WriteBytesExt};
use failure::{bail, Fallible as Result};
use std::borrow::Cow;
use std::cell::RefCell;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

// File storage format:
//
// Header: Magic string: 'appendstore\n'
//         Version:      BigEndian u32 (Current version: 1)
//
// Entries: Length:      BigEndian u32
//          Data:        "Length" bytes of data

const MAGIC_LEN: usize = 12;
const MAGIC: [u8; MAGIC_LEN] = *b"appendstore\n";
const VERSION: u32 = 1;
const HEADER_LEN: u64 = (MAGIC_LEN + 4) as u64;

/// Implementation of a store using file I/O to read and write blocks to a file.
pub struct FileStore {
    /// The underlying file.  This is stored in a RefCell so that we can seek during reads.
    file: RefCell<BufWriter<File>>,

    /// The position in the file to which new items will be written.
    position: u64,

    /// Whether the file handle is currently at the end of the file.  This is used to avoid seeking
    /// to the end each time a block is written, as seeking causes the BufWrite to flush, which
    /// hurts performance.  This is stored in a RefCell so that we can seek away from the end
    /// during reads.
    at_end: RefCell<bool>,

    /// True if the file is read-only.
    read_only: bool,

    /// Cache of data loaded from disk.  Used when iterating over the whole dirstate.
    cache: Option<Vec<u8>>,
}

impl FileStore {
    /// Create a new FileStore, overwriting any existing file.
    pub fn create<P: AsRef<Path>>(path: P) -> Result<FileStore> {
        let mut file = BufWriter::new(
            OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .open(&path)?,
        );
        file.write(&MAGIC)?;
        file.write_u32::<BigEndian>(VERSION)?;
        Ok(FileStore {
            file: RefCell::new(file),
            position: HEADER_LEN,
            at_end: RefCell::new(true),
            read_only: false,
            cache: None,
        })
    }

    /// Open an existing FileStore.  Attempts to open the file in read/write mode.  If write
    /// access is not permitted, falls back to opening the file in read-only mode.  When open
    /// in read-only mode, new blocks of data cannot be appended.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<FileStore> {
        let mut read_only = false;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .or_else(|_e| {
                read_only = true;
                OpenOptions::new().read(true).open(&path)
            })?;
        let mut file = BufWriter::new(file);

        // Check the file header is as expected.
        let mut buffer = [0; MAGIC_LEN];
        file.get_ref()
            .read_exact(&mut buffer)
            .map_err(|_e| ErrorKind::NotAStoreFile)?;
        if buffer != MAGIC {
            bail!(ErrorKind::NotAStoreFile);
        }
        let version = file.get_ref().read_u32::<BigEndian>()?;
        if version != VERSION {
            bail!(ErrorKind::UnsupportedVersion(version));
        }

        // Find the size of the file (and hence the position to write new blocks of data)
        // by seeking to the end.
        let position = file.seek(SeekFrom::End(0))?;

        Ok(FileStore {
            file: RefCell::new(file),
            position,
            at_end: RefCell::new(true),
            read_only,
            cache: None,
        })
    }

    pub fn cache(&mut self) -> Result<()> {
        if self.cache.is_none() {
            let file = self.file.get_mut();
            file.flush()?;
            file.seek(SeekFrom::Start(0))?;
            *self.at_end.get_mut() = false;
            let mut buffer = Vec::with_capacity(self.position as usize);
            unsafe {
                // This is safe as we've just allocated the buffer and are about to read into it.
                buffer.set_len(self.position as usize);
            }
            file.get_mut().read_exact(buffer.as_mut_slice())?;
            file.seek(SeekFrom::Start(self.position))?;
            *self.at_end.get_mut() = true;
            self.cache = Some(buffer);
        }
        Ok(())
    }

    pub fn position(&self) -> u64 {
        self.position
    }
}

impl Store for FileStore {
    fn append(&mut self, data: &[u8]) -> Result<BlockId> {
        if self.read_only {
            bail!(ErrorKind::ReadOnlyStore);
        }
        let id = BlockId(self.position);
        let file = self.file.get_mut();
        let at_end = self.at_end.get_mut();
        if !*at_end {
            file.seek(SeekFrom::Start(self.position))?;
            *at_end = true;
        }
        assert!(data.len() <= std::u32::MAX as usize, "data too long");
        file.write_u32::<BigEndian>(data.len() as u32)?;
        file.write_all(data)?;
        self.position += 4 + data.len() as u64;
        debug_assert!(self.position == file.seek(SeekFrom::End(0))?);
        Ok(id)
    }

    fn flush(&mut self) -> Result<()> {
        let file = self.file.get_mut();
        file.flush()?;
        file.get_mut().sync_all()?;
        Ok(())
    }
}

impl StoreView for FileStore {
    fn read<'a>(&'a self, id: BlockId) -> Result<Cow<'a, [u8]>> {
        // Check the ID is in range.
        if id.0 < HEADER_LEN || id.0 > self.position - 4 {
            bail!(ErrorKind::InvalidStoreId(id.0));
        }

        if let Some(ref cache) = self.cache {
            if (id.0) < cache.len() as u64 {
                if (id.0) > cache.len() as u64 - 4 {
                    // The ID falls in the last 3 bytes of the cache.  This is invalid.
                    bail!(ErrorKind::InvalidStoreId(id.0));
                }
                let header_start = id.0 as usize;
                let data_start = header_start + 4;
                let size = BigEndian::read_u32(&cache[header_start..data_start]) as usize;
                if size as u64 > cache.len() as u64 - data_start as u64 {
                    // The stored size of this block exceeds the number of bytes left in the
                    // cache.  We must have been given an invalid ID.
                    bail!(ErrorKind::InvalidStoreId(id.0));
                }
                return Ok(Cow::from(&cache[data_start..data_start + size]));
            }
        }

        // Get mutable access to the file, and seek to the right location.
        let mut file = self.file.borrow_mut();
        file.seek(SeekFrom::Start(id.0))?;
        *self.at_end.borrow_mut() = false;

        // Read the block of data from the file.
        let size = file.get_mut().read_u32::<BigEndian>()?;
        if size as u64 > self.position - id.0 {
            // The stored size of this block exceeds the number of bytes left in the file.  We
            // must have been given an invalid ID.
            bail!(ErrorKind::InvalidStoreId(id.0));
        }
        let mut buffer: Vec<u8> = Vec::with_capacity(size as usize);
        unsafe {
            // This is safe as we've just allocated the buffer and are about to read into it.
            buffer.set_len(size as usize);
        }
        file.get_mut().read_exact(&mut buffer[..])?;

        Ok(Cow::from(buffer))
    }
}

#[cfg(test)]
mod tests {
    use crate::filestore::FileStore;
    use crate::store::{BlockId, Store, StoreView};
    use std::fs;
    use std::io::Write;
    use tempdir::TempDir;

    #[test]
    fn goodpath() {
        let dir = TempDir::new("filestore_test").expect("create temp dir");
        let p = dir.path().join("store");
        let mut s = FileStore::create(&p).expect("create store");
        let id1 = s.append("data block 1".as_bytes()).expect("write block 1");
        let id2 = s
            .append("data block two".as_bytes())
            .expect("write block 2");
        s.flush().expect("flush");
        assert_eq!(s.read(id1).expect("read 1"), "data block 1".as_bytes());
        assert_eq!(s.read(id2).expect("read 2"), "data block two".as_bytes());
        drop(s);
        let mut s = FileStore::open(&p).expect("open store");
        assert_eq!(s.read(id1).expect("read 1"), "data block 1".as_bytes());
        assert_eq!(s.read(id2).expect("read 2"), "data block two".as_bytes());
        let id3 = s
            .append("third data block".as_bytes())
            .expect("write block 3");
        s.flush().expect("flush");
        drop(s);
        let s = FileStore::open(p.clone()).expect("open store");
        assert_eq!(s.read(id3).expect("read 3"), "third data block".as_bytes());
        assert_eq!(s.read(id2).expect("read 2"), "data block two".as_bytes());
        assert_eq!(s.read(id1).expect("read 1"), "data block 1".as_bytes());
        dir.close().expect("clean up temp dir");
    }

    #[test]
    fn readonly() {
        let dir = TempDir::new("filestore_test").expect("create temp dir");
        let p = dir.path().join("store");
        let mut s = FileStore::create(&p).expect("create store");
        let id1 = s.append("data block 1".as_bytes()).expect("write block 1");
        s.flush().expect("flush");
        drop(s);
        let mut perms = fs::metadata(&p).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&p, perms).unwrap();
        let mut s = FileStore::open(&p).expect("open store");
        assert_eq!(s.read(id1).expect("read 1"), "data block 1".as_bytes());
        assert_eq!(
            s.append("third data block".as_bytes())
                .unwrap_err()
                .to_string(),
            "store is read-only"
        );
    }

    #[test]
    fn empty_file() {
        let dir = TempDir::new("filestore_test").expect("create temp dir");
        let p = dir.path().join("store");
        let file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&p)
            .unwrap();
        drop(file);
        assert_eq!(
            FileStore::open(&p)
                .err()
                .expect("file should not be opened")
                .to_string(),
            "the provided store file is not a valid store file"
        );
    }

    #[test]
    fn invalid_file() {
        let dir = TempDir::new("filestore_test").expect("create temp dir");
        let p = dir.path().join("store");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&p)
            .unwrap();
        file.write(b"not a store file").unwrap();
        drop(file);
        assert_eq!(
            FileStore::open(&p)
                .err()
                .expect("file should not be opened")
                .to_string(),
            "the provided store file is not a valid store file"
        );
    }

    #[test]
    fn unsupported_version() {
        let dir = TempDir::new("filestore_test").expect("create temp dir");
        let p = dir.path().join("store");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&p)
            .unwrap();
        // Version 0 is not supported.
        file.write(b"appendstore\n\x00\x00\x00\x00").unwrap();
        drop(file);
        assert_eq!(
            FileStore::open(&p)
                .err()
                .expect("file should not be opened")
                .to_string(),
            "store file version not supported: 0"
        );
    }

    #[test]
    fn cache() {
        let dir = TempDir::new("filestore_test").expect("create temp dir");
        let p = dir.path().join("store");
        let mut s = FileStore::create(&p).expect("create store");
        let id1 = s.append("data block 1".as_bytes()).expect("write block 1");
        let id2 = s
            .append("data block two".as_bytes())
            .expect("write block 2");
        s.flush().expect("flush");
        assert_eq!(s.read(id1).expect("read 1"), "data block 1".as_bytes());
        assert_eq!(s.read(id2).expect("read 2"), "data block two".as_bytes());
        drop(s);
        let mut s = FileStore::open(&p).expect("open store");
        s.cache().expect("can cache");
        assert_eq!(s.read(id1).expect("read 1"), "data block 1".as_bytes());
        assert_eq!(s.read(id2).expect("read 2"), "data block two".as_bytes());
        let id3 = s
            .append("third data block".as_bytes())
            .expect("write block 3");
        s.flush().expect("flush");
        assert_eq!(s.read(id3).expect("read 3"), "third data block".as_bytes());
        assert_eq!(
            s.read(BlockId(id3.0 - 2)).unwrap_err().to_string(),
            format!("invalid store id: {}", id3.0 - 2)
        );
    }

    #[test]
    fn invalid_store_ids() {
        let dir = TempDir::new("filestore_test").expect("create temp dir");
        let p = dir.path().join("store");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&p)
            .unwrap();
        // Version 0 is not supported.
        file.write(b"appendstore\n\x00\x00\x00\x01\x00\x00\xff\xffdata")
            .unwrap();
        drop(file);
        let f = FileStore::open(&p).expect("file should be opened");
        // Store ID 2 is inside the header.
        assert_eq!(
            f.read(BlockId(2)).unwrap_err().to_string(),
            "invalid store id: 2"
        );
        // Store ID 16 has an invalid length.
        assert_eq!(
            f.read(BlockId(16)).unwrap_err().to_string(),
            "invalid store id: 16"
        );
        // Store ID 22 is within 4 bytes of the end of the file.
        assert_eq!(
            f.read(BlockId(22)).unwrap_err().to_string(),
            "invalid store id: 22"
        );
        // Store ID 64 is after the end of the file.
        assert_eq!(
            f.read(BlockId(64)).unwrap_err().to_string(),
            "invalid store id: 64"
        );
    }
}
