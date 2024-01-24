/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Read;
use std::sync::Arc;

use parking_lot::Mutex;

/// A crate::io::{Read,Write} impl backed by a Vec<u8>.
/// Intended to back a io.BytesIO like Python object.
#[derive(Clone)]
pub struct BufIO {
    buf: Arc<Mutex<std::io::Cursor<Vec<u8>>>>,
    dev_null: bool,
}

impl BufIO {
    /// Create a BufIO with `content` available for reading.
    pub fn with_content(content: Vec<u8>) -> Self {
        Self {
            buf: Arc::new(Mutex::new(std::io::Cursor::new(content))),
            dev_null: false,
        }
    }

    pub fn dev_null() -> Self {
        let mut buf = Self::with_content(Vec::new());
        buf.dev_null = true;
        buf
    }

    /// Current read position.
    pub fn position(&self) -> u64 {
        self.buf.lock().position()
    }

    /// Resize underlying vector to `size`, or `position` if `size` is not specified.
    /// Extending the vector fills with zeros. Reducing the vector's size below the current
    /// position will corrupt the buffer's state. Returns the new vector size.
    /// This mirrors Python io.BinaryIO's "truncate()".
    pub fn truncate(&self, size: Option<usize>) -> usize {
        let mut buf = self.buf.lock();

        let new_size = size.unwrap_or_else(|| buf.position() as usize);

        buf.get_mut().resize(new_size, 0);

        new_size
    }

    /// Accumulate bytes in `out` until `stop` returns `true`.
    /// `stop` takes the accumulated bytes so far, and `is_eof`.
    pub fn read_until(
        &self,
        out: &mut Vec<u8>,
        stop: impl Fn(/* so_far: */ &[u8], /* is_eof: */ bool) -> bool,
    ) -> std::io::Result<()> {
        let mut buf = self.buf.lock();
        while !stop(out, false) {
            let mut b = 0u8;
            if let Err(err) = buf.read_exact(std::slice::from_mut(&mut b)) {
                if err.kind() == std::io::ErrorKind::UnexpectedEof && stop(out, true) {
                    break;
                } else {
                    return Err(err);
                }
            }
            out.push(b);
        }
        Ok(())
    }

    /// Make a copy of the entire underlying vec.
    pub fn to_vec(&self) -> Vec<u8> {
        self.buf.lock().get_ref().clone()
    }
}

impl std::io::Seek for BufIO {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.buf.lock().seek(pos)
    }
}

impl std::io::Read for BufIO {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.buf.lock().read(buf)
    }
}

impl std::io::Write for BufIO {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.dev_null {
            return Ok(buf.len());
        }

        self.buf.lock().write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl crate::IsTty for BufIO {
    fn is_tty(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod test {
    use std::io::Read;
    use std::io::Seek;
    use std::io::Write;

    use crate::BufIO;

    #[test]
    fn test_buf_io_read() -> std::io::Result<()> {
        let mut buf = BufIO::with_content(b"hello".to_vec());
        assert_eq!(buf.position(), 0);

        let mut got = Vec::new();
        buf.read_to_end(&mut got)?;
        assert_eq!(got, b"hello");
        assert_eq!(buf.position(), 5);

        got.clear();
        buf.read_to_end(&mut got)?;
        assert_eq!(got, b"");
        assert_eq!(buf.to_vec(), b"hello");

        Ok(())
    }

    #[test]
    fn test_buf_io_seek() -> std::io::Result<()> {
        let mut buf = BufIO::with_content(b"hello".to_vec());

        buf.seek(std::io::SeekFrom::Start(1))?;
        let mut got = vec![0; 1];
        buf.read_exact(&mut got)?;
        assert_eq!(got, b"e");

        Ok(())
    }

    #[test]
    fn test_buf_io_truncate() -> std::io::Result<()> {
        let mut buf = BufIO::with_content(b"hello".to_vec());

        let mut got = vec![0; 1];
        buf.read_exact(&mut got)?;
        assert_eq!(got, b"h");
        assert_eq!(buf.position(), 1);

        assert_eq!(buf.truncate(None), 1);
        got.clear();
        buf.read_to_end(&mut got)?;
        assert!(got.is_empty());

        let mut buf = BufIO::with_content(b"a".to_vec());
        assert_eq!(buf.truncate(Some(2)), 2);
        got.clear();
        buf.read_to_end(&mut got)?;
        assert_eq!(got, b"a\0");

        Ok(())
    }

    #[test]
    fn test_buf_io_write() -> std::io::Result<()> {
        let mut buf = BufIO::with_content(Vec::new());
        write!(buf, "hello")?;
        assert_eq!(buf.position(), 5);

        let mut got = Vec::new();
        buf.read_to_end(&mut got)?;
        assert!(got.is_empty());

        Ok(())
    }

    #[test]
    fn test_buf_io_read_until() -> std::io::Result<()> {
        let mut buf = BufIO::with_content(b"hello".to_vec());

        let mut got = Vec::new();
        buf.read_until(&mut got, |so_far, _is_eof| so_far.last() == Some(&b'l'))?;
        assert_eq!(got, b"hel");

        buf.seek(std::io::SeekFrom::Start(0))?;
        got.clear();

        buf.read_until(&mut got, |_so_far, is_eof| is_eof)?;
        assert_eq!(got, b"hello");

        buf.seek(std::io::SeekFrom::Start(0))?;
        got.clear();

        assert!(buf.read_until(&mut got, |_so_far, _is_eof| false).is_err());
        assert_eq!(got, b"hello");

        Ok(())
    }

    #[test]
    fn test_io_buf_dev_null() -> std::io::Result<()> {
        let mut buf = BufIO::dev_null();

        assert_eq!(buf.write(b"hello")?, 5);
        assert_eq!(buf.position(), 0);
        assert!(buf.to_vec().is_empty());

        Ok(())
    }
}
