// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
use std::any::Any;
use std::io;

pub struct IO {
    pub input: Box<dyn Read>,
    pub output: Box<dyn Write>,
    pub error: Option<Box<dyn Write>>,
}

pub trait Read: io::Read + Any {
    fn as_any(&self) -> &dyn Any;
}

pub trait Write: io::Write + Any {
    fn as_any(&self) -> &dyn Any;
}

impl<T: io::Read + Any> Read for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl<T: io::Write + Any> Write for T {
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
        }
    }
}

impl Drop for IO {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}
