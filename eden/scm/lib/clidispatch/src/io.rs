/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use configparser::config::ConfigSet;
use parking_lot::Mutex;
use pipe::pipe;
use std::any::Any;
use std::io;
use std::mem;
use std::sync::Arc;
use std::thread::{spawn, JoinHandle};
use streampager::{config::InterfaceMode, Pager};

#[derive(Clone)]
pub struct IO {
    inner: Arc<Mutex<Inner>>,
}

/// Implements `io::Write` on the error stream.
#[derive(Clone)]
pub struct IOError(IO);

struct Inner {
    input: Box<dyn Read>,
    output: Box<dyn Write>,
    error: Option<Box<dyn Write>>,
    progress: Option<Box<dyn Write>>,

    pager_handle: Option<JoinHandle<streampager::Result<()>>>,
}

pub trait Read: io::Read + Any + Send + Sync {
    fn as_any(&self) -> &dyn Any;
}

pub trait Write: io::Write + Any + Send + Sync {
    fn as_any(&self) -> &dyn Any;
}

impl<T: io::Read + Any + Send + Sync> Read for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl<T: io::Write + Any + Send + Sync> Write for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

// Write to output.
impl io::Write for IO {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.lock().output.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.lock().flush()
    }
}

// Write to error.
impl io::Write for IOError {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut inner = self.0.inner.lock();
        if let Some(error) = inner.error.as_mut() {
            error.write(buf)
        } else {
            Ok(buf.len())
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut inner = self.0.inner.lock();
        if let Some(error) = inner.error.as_mut() {
            error.flush()?;
        }
        Ok(())
    }
}

impl IO {
    pub fn with_input<R>(&self, f: impl FnOnce(&dyn Read) -> R) -> R {
        f(self.inner.lock().input.as_ref())
    }

    pub fn with_output<R>(&self, f: impl FnOnce(&dyn Write) -> R) -> R {
        f(self.inner.lock().output.as_ref())
    }

    pub fn with_error<R>(&self, f: impl FnOnce(Option<&dyn Write>) -> R) -> R {
        f(self.inner.lock().error.as_deref())
    }

    /// Returns a clonable value that impls [`io::Write`] to `error` stream.
    /// The output is associated with the `IO` so if the `IO` starts a pager,
    /// the error stream will be properly redirected to the pager.
    pub fn error(&self) -> IOError {
        IOError(self.clone())
    }

    pub fn new<IS, OS, ES>(input: IS, output: OS, error: Option<ES>) -> Self
    where
        IS: Read + 'static,
        OS: Write + 'static,
        ES: Write + 'static,
    {
        let inner = Inner {
            input: Box::new(input),
            output: Box::new(output),
            error: error.map(|e| Box::new(e) as Box<dyn Write>),
            progress: None,
            pager_handle: None,
        };
        Self {
            inner: Arc::new(Mutex::new(inner)),
        }
    }

    pub fn write(&mut self, data: impl AsRef<[u8]>) -> io::Result<()> {
        let data = data.as_ref();
        self.inner.lock().output.write_all(data)?;
        Ok(())
    }

    pub fn write_err(&mut self, data: impl AsRef<[u8]>) -> io::Result<()> {
        let data = data.as_ref();
        let mut inner = self.inner.lock();
        if let Some(ref mut error) = inner.error {
            error.write_all(data)?;
        } else {
            inner.output.write_all(data)?;
        }
        Ok(())
    }

    pub fn write_progress(&mut self, data: impl AsRef<[u8]>) -> io::Result<()> {
        let data = data.as_ref();
        let mut inner = self.inner.lock();
        if let Some(ref mut progress) = inner.progress {
            progress.write_all(data)?;
        }
        Ok(())
    }

    pub fn stdio() -> Self {
        let inner = Inner {
            input: Box::new(io::stdin()),
            output: Box::new(io::stdout()),
            error: Some(Box::new(io::stderr())),
            progress: None,
            pager_handle: None,
        };
        Self {
            inner: Arc::new(Mutex::new(inner)),
        }
    }

    pub fn start_pager(&mut self, config: &ConfigSet) -> io::Result<()> {
        let mut inner = self.inner.lock();
        if inner.pager_handle.is_some() {
            return Ok(());
        }

        let mut pager =
            Pager::new_using_stdio().map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        // Configure the pager.
        // The Hybrid mode is similar to "-FX" from "less".
        let mut interface_mode = InterfaceMode::Hybrid;
        // Similar to "less" default.
        let mut scroll_past_eof = false;
        if let Some(mode_str) = config.get("pager", "interface") {
            let mode = InterfaceMode::from(mode_str.as_ref());
            interface_mode = mode;
        }
        if let Ok(Some(past_eof)) = config.get_opt("pager", "scroll-past-eof") {
            scroll_past_eof = past_eof;
        }
        pager
            .set_scroll_past_eof(scroll_past_eof)
            .set_interface_mode(interface_mode);

        let (out_read, out_write) = pipe();
        let (err_read, err_write) = pipe();
        let (prg_read, prg_write) = pipe();

        inner.flush()?;
        inner.output = Box::new(out_write);
        inner.error = Some(Box::new(err_write));
        inner.progress = Some(Box::new(prg_write));

        inner.pager_handle = Some(spawn(|| {
            pager
                .add_output_stream(out_read, "")?
                .add_error_stream(err_read, "")?
                .set_progress_stream(prg_read);
            pager.run()?;
            Ok(())
        }));

        Ok(())
    }
}

impl Inner {
    pub fn flush(&mut self) -> io::Result<()> {
        self.output.flush()?;
        if let Some(ref mut error) = self.error {
            error.flush()?;
        }
        Ok(())
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        let _ = self.flush();
        // Drop the output and error. This sends EOF to pager.
        self.output = Box::new(Vec::new());
        self.error = None;
        self.progress = None;
        // Wait for the pager.
        let mut handle = None;
        mem::swap(&mut handle, &mut self.pager_handle);
        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }
}
