/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{
    cell::{RefCell, RefMut},
    fmt::Debug,
    io::{self, BufWriter, Write},
};

use failure::Fallible as Result;

/// A `PackWriter` will buffers all the writes to `T` and count the total number of bytes written.
pub struct PackWriter<T: Write> {
    data: RefCell<BufWriter<T>>,
    bytes_written: u64,
}

impl<T: 'static + Write + Debug + Send + Sync> PackWriter<T> {
    pub fn new(value: T) -> PackWriter<T> {
        PackWriter {
            data: RefCell::new(BufWriter::new(value)),
            bytes_written: 0,
        }
    }

    /// Flush the buffered data to the underlying writer.
    pub fn flush_inner(&self) -> Result<()> {
        let ret = self.data.try_borrow_mut()?.flush()?;
        Ok(ret)
    }

    /// Return the number of bytes written. Note that due to the buffering nature of a
    /// `PackWriter`, not all the data may have reached the underlying writer.
    pub fn bytes_written(&self) -> u64 {
        self.bytes_written
    }

    /// Return a mutable reference on the underlying writer. It's not recommended to write to it.
    pub fn get_mut(&self) -> RefMut<T> {
        let cell = self.data.borrow_mut();
        RefMut::map(cell, |w| w.get_mut())
    }

    /// Flush the buffered data and return the underlying writer.
    pub fn into_inner(self) -> Result<T> {
        let ret = self.data.into_inner().into_inner()?;
        Ok(ret)
    }
}

impl<T: Write> Write for PackWriter<T> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let ret = self.data.get_mut().write(buf)?;
        self.bytes_written += ret as u64;
        Ok(ret)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.data.get_mut().flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use byteorder::{ReadBytesExt, WriteBytesExt};
    use tempfile::tempfile;

    use std::io::{Seek, SeekFrom};

    #[test]
    fn test_bytes_written() {
        let mut file = PackWriter::new(tempfile().unwrap());
        file.write_u8(10).unwrap();

        assert_eq!(file.bytes_written(), 1);
    }

    #[test]
    fn test_write() {
        let mut file = PackWriter::new(tempfile().unwrap());
        file.write_u8(10).unwrap();

        // into_inner() flushes its internal buffer.
        let mut inner = file.into_inner().unwrap();
        inner.seek(SeekFrom::Start(0)).unwrap();
        let data = inner.read_u8().unwrap();
        assert_eq!(data, 10);
    }

    #[test]
    fn test_read_without_drain() {
        let mut file = PackWriter::new(tempfile().unwrap());
        file.write_u8(10).unwrap();

        let mut inner = file.get_mut();
        inner.seek(SeekFrom::Start(0)).unwrap();
        assert!(inner.read_u8().is_err());
    }

    #[test]
    fn test_flush_inner() {
        let mut file = PackWriter::new(tempfile().unwrap());
        file.write_u8(10).unwrap();
        file.flush_inner().unwrap();

        let mut inner = file.get_mut();
        inner.seek(SeekFrom::Start(0)).unwrap();
        let data = inner.read_u8().unwrap();
        assert_eq!(data, 10);
    }
}
