/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cell::Cell;
use std::fs::File;
use std::io as std_io;
use std::sync::RwLock;

use ::vfs::LiteMetadata;
use cpython::*;
use cpython_ext::PyNone;
use cpython_ext::ResultPyErrExt;
use cpython_ext::SimplePyBuf;

use crate::metadata::metadata;

pub trait IOObject: Send + Sync {
    fn as_read(&mut self) -> Option<&mut dyn ::io::Read> {
        None
    }

    fn as_write(&mut self) -> Option<&mut dyn ::io::Write> {
        None
    }

    fn as_file(&mut self) -> Option<&File> {
        None
    }

    fn as_seek(&mut self) -> Option<&mut (dyn std_io::Seek + Send)> {
        None
    }

    fn close(&mut self) -> std_io::Result<()> {
        Ok(())
    }
}

py_class!(pub class PyRustIO |py| {
    data inner: RwLock<Option<Box<dyn IOObject>>>;
    data is_closed: Cell<bool>;

    /// Read at most `n` bytes from the input.
    /// If `n` is negative, read everything till the end.
    def read(&self, n: i64 = -1) -> PyResult<PyBytes> {
        self.check_open(py)?;
        let inner = self.inner(py);
        let mut io = lock_write(py, inner)?;
        let io = match io.as_mut().and_then(|io| io.as_read()) {
            Some(io) => io,
            None => return Err(not_readable(py)),
        };
        let mut buf = Vec::<u8>::new();
        if n < 0 {
            py.allow_threads(|| io.read_to_end(&mut buf)).map_pyerr(py)?;
        } else if n == 0 {
            // Avoid BufReader::read(), which can block filling its buffer.
        } else {
            buf.resize(n as usize, 0u8);
            let read_bytes = py.allow_threads(|| io.read(&mut buf)).map_pyerr(py)?;
            buf.truncate(read_bytes);
        }
        Ok(PyBytes::new(py, &buf))
    }

    def readline(&self) -> PyResult<PyBytes> {
        self.check_open(py)?;
        let inner = self.inner(py);
        let mut io = lock_write(py, inner)?;
        let io = match io.as_mut().and_then(|io| io.as_read()) {
            Some(io) => io,
            None => return Err(not_readable(py)),
        };
        let mut buf = Vec::<u8>::new();
        py.allow_threads(|| read_line(io, &mut buf)).map_pyerr(py)?;
        Ok(PyBytes::new(py, &buf))
    }

    def readlines(&self, hint: i64 = -1) -> PyResult<Vec<PyBytes>> {
        self.check_open(py)?;
        let inner = self.inner(py);
        let mut io = lock_write(py, inner)?;
        let io = match io.as_mut().and_then(|io| io.as_read()) {
            Some(io) => io,
            None => return Err(not_readable(py)),
        };
        let lines = py.allow_threads(|| read_lines(io, hint)).map_pyerr(py)?;
        Ok(lines.iter().map(|line| PyBytes::new(py, line)).collect())
    }

    def write(&self, bytes: PyObject) -> PyResult<usize> {
        self.check_open(py)?;
        let inner = self.inner(py);
        let mut io = lock_write(py, inner)?;
        let io = match io.as_mut().and_then(|io| io.as_write()) {
            Some(io) => io,
            None => return Err(not_writable(py)),
        };
        let bytes = SimplePyBuf::<u8>::try_new(py, &bytes)?;
        let bytes = bytes.as_ref();
        py.allow_threads(|| io.write_all(bytes)).map_pyerr(py)?;
        Ok(bytes.len())
    }

    def writelines(&self, lines: Vec<PyObject>) -> PyResult<PyNone> {
        self.check_open(py)?;
        let inner = self.inner(py);
        let mut io = lock_write(py, inner)?;
        let io = match io.as_mut().and_then(|io| io.as_write()) {
            Some(io) => io,
            None => return Err(not_writable(py)),
        };
        let lines: Vec<SimplePyBuf<u8>> = lines
            .iter()
            .map(|line| SimplePyBuf::<u8>::try_new(py, line))
            .collect::<PyResult<_>>()?;
        let lines: Vec<&[u8]> = lines.iter().map(|line| line.as_ref()).collect();
        py.allow_threads(|| write_lines(io, &lines)).map_pyerr(py)?;
        Ok(PyNone)
    }

    def flush(&self) -> PyResult<PyNone> {
        self.check_open(py)?;
        let inner = self.inner(py);
        let mut io = lock_write(py, inner)?;
        let io = match io.as_mut().and_then(|io| io.as_write()) {
            Some(io) => io,
            None => return Ok(PyNone),
        };
        py.allow_threads(|| io.flush()).map_pyerr(py)?;
        Ok(PyNone)
    }

    def metadata(&self) -> PyResult<metadata> {
        self.check_open(py)?;
        let inner = self.inner(py);
        let mut io = lock_write(py, inner)?;
        let file = match io.as_mut().and_then(|io| io.as_file()) {
            Some(file) => file,
            None => {
                return Err(unsupported_operation(py, "metadata")?);
            }
        };
        let meta: LiteMetadata = py.allow_threads(|| file.metadata()).map_pyerr(py)?.into();
        metadata::create_instance(py, meta)
    }

    def seek(&self, offset: i64, whence: i8 = 0) -> PyResult<u64> {
        self.check_open(py)?;
        let inner = self.inner(py);
        let mut io = lock_write(py, inner)?;
        let seek = match io.as_mut().and_then(|io| io.as_seek()) {
            Some(seek) => seek,
            None => {
                return Err(unsupported_operation(py, "underlying stream is not seekable")?);
            }
        };
        let pos = match whence {
            0 => std_io::SeekFrom::Start(offset.try_into().map_pyerr(py)?),
            1 => std_io::SeekFrom::Current(offset),
            2 => std_io::SeekFrom::End(offset),
            _ => {
                return Err(PyErr::new::<exc::ValueError, _>(
                    py,
                    format!("invalid whence: {whence}"),
                ));
            }
        };
        py.allow_threads(|| std_io::Seek::seek(seek, pos)).map_pyerr(py)
    }

    def tell(&self) -> PyResult<u64> {
        self.check_open(py)?;
        let inner = self.inner(py);
        let mut io = lock_write(py, inner)?;
        let seek = match io.as_mut().and_then(|io| io.as_seek()) {
            Some(seek) => seek,
            None => {
                return Err(unsupported_operation(py, "underlying stream is not seekable")?);
            }
        };
        py.allow_threads(|| std_io::Seek::stream_position(seek)).map_pyerr(py)
    }

    def isatty(&self) -> PyResult<bool> {
        let inner = self.inner(py);
        let mut io = lock_write(py, inner)?;
        let Some(io) = io.as_mut() else {
            return Ok(false);
        };
        if let Some(io) = io.as_write() {
            return Ok(io.is_tty());
        }
        match io.as_read() {
            Some(io) => Ok(io.is_tty()),
            None => Ok(false),
        }
    }

    def isstdin(&self) -> PyResult<bool> {
        let inner = self.inner(py);
        let mut io = lock_write(py, inner)?;
        match io.as_mut().and_then(|io| io.as_read()) {
            Some(io) => Ok(io.is_stdin()),
            None => Ok(false),
        }
    }

    def isstdout(&self) -> PyResult<bool> {
        let inner = self.inner(py);
        let mut io = lock_write(py, inner)?;
        match io.as_mut().and_then(|io| io.as_write()) {
            Some(io) => Ok(io.is_stdout()),
            None => Ok(false),
        }
    }

    def isstderr(&self) -> PyResult<bool> {
        let inner = self.inner(py);
        let mut io = lock_write(py, inner)?;
        match io.as_mut().and_then(|io| io.as_write()) {
            Some(io) => Ok(io.is_stderr()),
            None => Ok(false),
        }
    }

    def close(&self) -> PyResult<PyNone> {
        let inner = self.inner(py);
        let mut io = lock_write(py, inner)?;
        if let Some(io) = io.as_mut() {
            py.allow_threads(|| io.close()).map_pyerr(py)?;
        }
        io.take();
        self.is_closed(py).set(true);
        Ok(PyNone)
    }

    def __enter__(&self) -> PyResult<Self> {
        Ok(self.clone_ref(py))
    }

    def __exit__(&self, ty: Option<PyType>, _value: PyObject, _traceback: PyObject) -> PyResult<bool> {
        if ty.is_none() {
            self.close(py)?;
        }
        Ok(false)
    }

    @property
    def closed(&self) -> PyResult<bool> {
        Ok(self.is_closed(py).get())
    }

    def readable(&self) -> PyResult<bool> {
        let inner = self.inner(py);
        let mut io = lock_write(py, inner)?;
        Ok(io.as_mut().and_then(|io| io.as_read()).is_some())
    }

    def seekable(&self) -> PyResult<bool> {
        let inner = self.inner(py);
        let mut io = lock_write(py, inner)?;
        Ok(io.as_mut().and_then(|io| io.as_seek()).is_some())
    }

    def writable(&self) -> PyResult<bool> {
        let inner = self.inner(py);
        let mut io = lock_write(py, inner)?;
        Ok(io.as_mut().and_then(|io| io.as_write()).is_some())
    }

    def fileno(&self) -> PyResult<usize> {
        // Emulated. Might be inaccurate.
        if self.isstdin(py)? {
            Ok(0)
        } else if self.isstdout(py)? {
            Ok(1)
        } else if self.isstderr(py)? {
            Ok(2)
        } else {
            Err(unsupported_operation(py, "fileno is not supported")?)
        }
    }

    def __iter__(&self) -> PyResult<PyObject> {
        // For simplicity, just read the entire file, assuming the callsite will consume
        // the iterable, before calling otheer file read methods.
        self.readlines(py, -1)?
            .to_py_object(py)
            .into_object()
            .call_method(py, "__iter__", NoArgs, None)
    }
});

pub(crate) fn unsupported_operation(py: Python, message: &str) -> PyResult<PyErr> {
    Ok(PyErr::from_instance(
        py,
        py.import("io")?
            .get(py, "UnsupportedOperation")?
            .call(py, (message,), None)?,
    ))
}

impl PyRustIO {
    fn check_open(&self, py: Python) -> PyResult<PyNone> {
        if self.is_closed(py).get() {
            Err(std_io::Error::new(
                std_io::ErrorKind::NotConnected,
                "stream was closed",
            ))
            .map_pyerr(py)
        } else {
            Ok(PyNone)
        }
    }
}

fn lock_write<'a>(
    py: Python,
    lock: &'a RwLock<Option<Box<dyn IOObject>>>,
) -> PyResult<std::sync::RwLockWriteGuard<'a, Option<Box<dyn IOObject>>>> {
    py.allow_threads(|| lock.write())
        .map_err(|_| PyErr::new::<exc::RuntimeError, _>(py, "rust IO lock was poisoned"))
}

fn read_lines(io: &mut dyn ::io::Read, hint: i64) -> std_io::Result<Vec<Vec<u8>>> {
    let mut lines = Vec::new();
    let mut total_read = 0usize;

    loop {
        let mut line = Vec::new();
        let read_bytes = read_line(io, &mut line)?;
        if read_bytes == 0 {
            return Ok(lines);
        }
        total_read += read_bytes;
        lines.push(line);
        if hint > 0 && total_read >= hint as usize {
            return Ok(lines);
        }
    }
}

fn write_lines(io: &mut dyn ::io::Write, lines: &[&[u8]]) -> std_io::Result<()> {
    let mut writer = std_io::BufWriter::new(io);
    for line in lines {
        std_io::Write::write_all(&mut writer, line)?;
    }
    std_io::Write::flush(&mut writer)
}

fn read_line(io: &mut dyn ::io::Read, buf: &mut Vec<u8>) -> std_io::Result<usize> {
    // This reads one byte at a time from the Read trait. It is still efficient
    // for IOObjects that return a buffered reader from as_read().
    let mut read_bytes = 0;
    let mut byte = [0u8; 1];
    loop {
        let count = io.read(&mut byte)?;
        if count == 0 {
            return Ok(read_bytes);
        }
        read_bytes += count;
        buf.push(byte[0]);
        if byte[0] == b'\n' {
            return Ok(read_bytes);
        }
    }
}

fn not_readable(py: Python) -> PyErr {
    PyErr::new::<exc::IOError, _>(py, "stream is not readable")
}

fn not_writable(py: Python) -> PyErr {
    PyErr::new::<exc::IOError, _>(py, "stream is not writable")
}

struct ReadObject<R> {
    inner: std_io::BufReader<R>,
}

impl<R: ::io::Read + 'static> IOObject for ReadObject<R> {
    fn as_read(&mut self) -> Option<&mut dyn ::io::Read> {
        Some(&mut self.inner)
    }
}

struct WriteObject<W> {
    inner: W,
}

impl<W: ::io::Write + 'static> IOObject for WriteObject<W> {
    fn as_write(&mut self) -> Option<&mut dyn ::io::Write> {
        Some(&mut self.inner)
    }

    fn close(&mut self) -> std_io::Result<()> {
        self.inner.flush()
    }
}

struct FileLikeObject<T> {
    inner: std_io::BufReader<FileLikeInner<T>>,
}

impl<T> FileLikeObject<T> {
    fn new(
        mut inner: T,
        file_fn: fn(&mut T) -> &mut File,
        close_fn: fn(&mut T) -> std_io::Result<()>,
    ) -> Self {
        let is_tty = ::io::IsTty::is_tty(file_fn(&mut inner));
        Self {
            inner: std_io::BufReader::new(FileLikeInner {
                inner,
                file_fn,
                close_fn,
                is_tty,
            }),
        }
    }

    fn sync_position(&mut self) -> std_io::Result<()> {
        if !self.inner.buffer().is_empty() {
            std_io::Seek::seek(&mut self.inner, std_io::SeekFrom::Current(0))?;
        }
        Ok(())
    }
}

impl<T> ::io::IsTty for FileLikeObject<T> {
    fn is_tty(&self) -> bool {
        ::io::IsTty::is_tty(self.inner.get_ref())
    }
}

impl<T> std_io::Write for FileLikeObject<T> {
    fn write(&mut self, buf: &[u8]) -> std_io::Result<usize> {
        self.sync_position()?;
        std_io::Write::write(self.inner.get_mut(), buf)
    }

    fn flush(&mut self) -> std_io::Result<()> {
        self.sync_position()?;
        std_io::Write::flush(self.inner.get_mut())
    }
}

struct FileLikeInner<T> {
    inner: T,
    file_fn: fn(&mut T) -> &mut File,
    close_fn: fn(&mut T) -> std_io::Result<()>,
    is_tty: bool,
}

impl<T> FileLikeInner<T> {
    fn file(&mut self) -> &mut File {
        (self.file_fn)(&mut self.inner)
    }

    fn close(&mut self) -> std_io::Result<()> {
        (self.close_fn)(&mut self.inner)
    }
}

impl<T> ::io::IsTty for FileLikeInner<T> {
    fn is_tty(&self) -> bool {
        self.is_tty
    }
}

impl<T> std_io::Read for FileLikeInner<T> {
    fn read(&mut self, buf: &mut [u8]) -> std_io::Result<usize> {
        std_io::Read::read(self.file(), buf)
    }
}

impl<T> std_io::Write for FileLikeInner<T> {
    fn write(&mut self, buf: &[u8]) -> std_io::Result<usize> {
        std_io::Write::write(self.file(), buf)
    }

    fn flush(&mut self) -> std_io::Result<()> {
        std_io::Write::flush(self.file())
    }
}

impl<T> std_io::Seek for FileLikeInner<T> {
    fn seek(&mut self, pos: std_io::SeekFrom) -> std_io::Result<u64> {
        std_io::Seek::seek(self.file(), pos)
    }
}

impl<T> IOObject for FileLikeObject<T>
where
    T: Send + Sync + 'static,
{
    fn as_read(&mut self) -> Option<&mut dyn ::io::Read> {
        Some(&mut self.inner)
    }

    fn as_write(&mut self) -> Option<&mut dyn ::io::Write> {
        Some(self)
    }

    fn as_file(&mut self) -> Option<&File> {
        Some(self.inner.get_mut().file())
    }

    fn as_seek(&mut self) -> Option<&mut (dyn std_io::Seek + Send)> {
        Some(&mut self.inner)
    }

    fn close(&mut self) -> std_io::Result<()> {
        self.inner.get_mut().close()
    }
}

pub(crate) fn wrap_io_object(py: Python, io: impl IOObject + 'static) -> PyResult<PyRustIO> {
    PyRustIO::create_instance(py, RwLock::new(Some(Box::new(io))), Cell::new(false))
}

pub fn wrap_file_like<T>(
    py: Python,
    obj: T,
    file_fn: fn(&mut T) -> &mut File,
    close_fn: fn(&mut T) -> std_io::Result<()>,
) -> PyResult<PyRustIO>
where
    T: Send + Sync + 'static,
{
    wrap_io_object(py, FileLikeObject::new(obj, file_fn, close_fn))
}

/// Wrap a Rust Write trait object into a Python object.
pub(crate) fn wrap_rust_write(py: Python, w: impl ::io::Write + 'static) -> PyResult<PyRustIO> {
    wrap_io_object(py, WriteObject { inner: w })
}

/// Wrap a Rust Read trait object into a Python object.
pub(crate) fn wrap_rust_read(py: Python, r: impl ::io::Read + 'static) -> PyResult<PyRustIO> {
    wrap_io_object(
        py,
        ReadObject {
            inner: std_io::BufReader::new(r),
        },
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    use super::*;

    fn file_as_file(file: &mut File) -> &mut File {
        file
    }

    fn close_file(file: &mut File) -> std_io::Result<()> {
        std_io::Write::flush(file)
    }

    fn tempfile_with_content(content: &[u8]) -> File {
        let mut file = tempfile::tempfile().expect("failed to create temporary file");
        std_io::Write::write_all(&mut file, content).expect("failed to write test file");
        std_io::Seek::seek(&mut file, std_io::SeekFrom::Start(0))
            .expect("failed to rewind test file");
        file
    }

    struct FailFirstClose {
        close_count: Arc<AtomicUsize>,
    }

    struct CountClose {
        close_count: Arc<AtomicUsize>,
    }

    struct CountWrite {
        write_count: Arc<AtomicUsize>,
        bytes: Arc<Mutex<Vec<u8>>>,
    }

    impl IOObject for CountClose {
        fn close(&mut self) -> std_io::Result<()> {
            self.close_count.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    impl ::io::IsTty for CountWrite {
        fn is_tty(&self) -> bool {
            false
        }
    }

    impl std_io::Write for CountWrite {
        fn write(&mut self, buf: &[u8]) -> std_io::Result<usize> {
            self.write_count.fetch_add(1, Ordering::Relaxed);
            self.bytes
                .lock()
                .expect("failed to lock written bytes")
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std_io::Result<()> {
            Ok(())
        }
    }

    impl IOObject for CountWrite {
        fn as_write(&mut self) -> Option<&mut dyn ::io::Write> {
            Some(self)
        }
    }

    impl IOObject for FailFirstClose {
        fn close(&mut self) -> std_io::Result<()> {
            match self.close_count.fetch_add(1, Ordering::Relaxed) {
                0 => Err(std_io::Error::other("close failed")),
                _ => Ok(()),
            }
        }
    }

    #[test]
    fn close_error_keeps_inner_open_for_retry() {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let close_count = Arc::new(AtomicUsize::new(0));
        let io = wrap_io_object(
            py,
            FailFirstClose {
                close_count: close_count.clone(),
            },
        )
        .expect("failed to create PyRustIO");

        assert!(io.close(py).is_err());
        assert_eq!(close_count.load(Ordering::Relaxed), 1);
        assert!(!io.closed(py).expect("failed to read closed state"));

        io.close(py).expect("second close should retry and succeed");
        assert_eq!(close_count.load(Ordering::Relaxed), 2);
        assert!(io.closed(py).expect("failed to read closed state"));
    }

    #[test]
    fn context_manager_exit_closes_without_exception() {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let close_count = Arc::new(AtomicUsize::new(0));
        let io = wrap_io_object(
            py,
            CountClose {
                close_count: close_count.clone(),
            },
        )
        .expect("failed to create PyRustIO");

        let suppress = io
            .__exit__(py, None, py.None(), py.None())
            .expect("failed to exit context manager");
        assert!(!suppress);
        assert_eq!(close_count.load(Ordering::Relaxed), 1);
        assert!(io.closed(py).expect("failed to read closed state"));
    }

    #[test]
    fn context_manager_exit_keeps_open_with_exception() {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let close_count = Arc::new(AtomicUsize::new(0));
        let io = wrap_io_object(
            py,
            CountClose {
                close_count: close_count.clone(),
            },
        )
        .expect("failed to create PyRustIO");

        let suppress = io
            .__exit__(
                py,
                Some(py.get_type::<exc::RuntimeError>()),
                py.None(),
                py.None(),
            )
            .expect("failed to exit context manager");
        assert!(!suppress);
        assert_eq!(close_count.load(Ordering::Relaxed), 0);
        assert!(!io.closed(py).expect("failed to read closed state"));

        io.close(py).expect("failed to close file");
        assert_eq!(close_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn writelines_buffers_small_writes() {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let write_count = Arc::new(AtomicUsize::new(0));
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let io = wrap_io_object(
            py,
            CountWrite {
                write_count: write_count.clone(),
                bytes: bytes.clone(),
            },
        )
        .expect("failed to create PyRustIO");

        io.writelines(
            py,
            vec![
                PyBytes::new(py, b"ab").into_object(),
                PyBytes::new(py, b"cd").into_object(),
                PyBytes::new(py, b"ef").into_object(),
            ],
        )
        .expect("failed to write lines");

        assert_eq!(write_count.load(Ordering::Relaxed), 1);
        assert_eq!(
            *bytes.lock().expect("failed to lock written bytes"),
            b"abcdef".to_vec()
        );
    }

    #[test]
    fn file_like_readline_keeps_buffered_bytes() {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let file = tempfile_with_content(b"abc\ndef");
        let io =
            wrap_file_like(py, file, file_as_file, close_file).expect("failed to create PyRustIO");

        let line = io.readline(py).expect("failed to read line");
        assert_eq!(line.data(py), b"abc\n");

        let rest = io.read(py, -1).expect("failed to read rest");
        assert_eq!(rest.data(py), b"def");
        io.close(py).expect("failed to close file");
    }

    #[test]
    fn file_like_seek_uses_buffered_logical_position() {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let file = tempfile_with_content(b"abc\ndef");
        let io =
            wrap_file_like(py, file, file_as_file, close_file).expect("failed to create PyRustIO");

        let first = io.read(py, 1).expect("failed to read first byte");
        assert_eq!(first.data(py), b"a");
        assert_eq!(io.tell(py).expect("failed to tell position"), 1);
        assert_eq!(
            io.seek(py, 0, 1)
                .expect("failed to seek to current position"),
            1
        );

        assert_eq!(io.seek(py, 2, 0).expect("failed to seek"), 2);
        let line = io.readline(py).expect("failed to read line");
        assert_eq!(line.data(py), b"c\n");
    }

    #[test]
    fn file_like_write_after_buffered_read_uses_logical_position() {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let file = tempfile_with_content(b"abc\ndef");
        let mut check_file = file.try_clone().expect("failed to clone test file");
        let io =
            wrap_file_like(py, file, file_as_file, close_file).expect("failed to create PyRustIO");

        let first = io.read(py, 1).expect("failed to read first byte");
        assert_eq!(first.data(py), b"a");
        assert_eq!(
            io.write(py, PyBytes::new(py, b"Z").into_object())
                .expect("failed to write replacement byte"),
            1
        );
        io.close(py).expect("failed to close file");

        let mut contents = Vec::new();
        std_io::Seek::seek(&mut check_file, std_io::SeekFrom::Start(0))
            .expect("failed to rewind test file");
        std_io::Read::read_to_end(&mut check_file, &mut contents)
            .expect("failed to read test file");
        assert_eq!(contents, b"aZc\ndef");
    }
}
