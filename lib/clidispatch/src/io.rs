// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
use std::default::Default;
use std::io::{self, Read, Write};

pub struct IO {
    output: Box<dyn Write>,
    input: Box<dyn Read>,
}

impl IO {
    fn new<OS, IS>(output: OS, input: IS) -> Self
    where
        OS: Write + 'static,
        IS: Read + 'static,
    {
        IO {
            output: Box::new(output),
            input: Box::new(input),
        }
    }

    pub fn write_str(&mut self, msg: impl AsRef<[u8]>) -> io::Result<()> {
        self.write(msg.as_ref())
    }

    pub fn write(&mut self, msg: &[u8]) -> io::Result<()> {
        self.output.write_all(msg)?;
        self.output.flush()?;
        Ok(())
    }
}

impl Default for IO {
    fn default() -> Self {
        IO {
            output: Box::new(io::stdout()),
            input: Box::new(io::stdin()),
        }
    }
}
