/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use pipe::pipe;
use std::any::Any;
use std::io;
use std::mem;
use std::thread::{spawn, JoinHandle};
use streampager::Pager;

pub struct IO {
    pub input: Box<dyn Read>,
    pub output: Box<dyn Write>,
    pub error: Option<Box<dyn Write>>,

    pager_handle: Option<JoinHandle<streampager::Result<()>>>,
}

pub trait Read: io::Read + Any + Send {
    fn as_any(&self) -> &dyn Any;
}

pub trait Write: io::Write + Any + Send {
    fn as_any(&self) -> &dyn Any;
}

impl<T: io::Read + Any + Send> Read for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl<T: io::Write + Any + Send> Write for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl IO {
    pub fn new<IS, OS, ES>(input: IS, output: OS, error: Option<ES>) -> Self
    where
        IS: Read + 'static,
        OS: Write + 'static,
        ES: Write + 'static,
    {
        IO {
            input: Box::new(input),
            output: Box::new(output),
            error: error.map(|e| Box::new(e) as Box<dyn Write>),
            pager_handle: None,
        }
    }

    pub fn write(&mut self, data: impl AsRef<[u8]>) -> io::Result<()> {
        let data = data.as_ref();
        self.output.write_all(data)?;
        Ok(())
    }

    pub fn write_err(&mut self, data: impl AsRef<[u8]>) -> io::Result<()> {
        let data = data.as_ref();
        if let Some(ref mut error) = self.error {
            error.write_all(data)?;
        } else {
            self.output.write_all(data)?;
        }
        Ok(())
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.output.flush()?;
        if let Some(ref mut error) = self.error {
            error.flush()?;
        }
        Ok(())
    }

    pub fn stdio() -> Self {
        IO {
            input: Box::new(io::stdin()),
            output: Box::new(io::stdout()),
            error: Some(Box::new(io::stderr())),
            pager_handle: None,
        }
    }

    pub fn start_pager(&mut self) -> io::Result<()> {
        if self.pager_handle.is_some() {
            return Ok(());
        }

        let mut pager =
            Pager::new_using_stdio().map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let (out_read, out_write) = pipe();
        let (err_read, err_write) = pipe();

        self.flush()?;
        self.output = Box::new(out_write);
        self.error = Some(Box::new(err_write));

        self.pager_handle = Some(spawn(|| {
            pager
                .add_output_stream(out_read, "")?
                .add_error_stream(err_read, "")?;
            pager.run()?;
            Ok(())
        }));

        Ok(())
    }
}

impl Drop for IO {
    fn drop(&mut self) {
        let _ = self.flush();
        // Drop the output and error. This sends EOF to pager.
        self.output = Box::new(Vec::new());
        self.error = None;
        // Wait for the pager.
        let mut handle = None;
        mem::swap(&mut handle, &mut self.pager_handle);
        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }
}
