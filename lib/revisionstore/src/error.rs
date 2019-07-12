// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::fmt;

use failure::{Backtrace, Context, Error, Fail};

#[derive(Debug)]
pub struct KeyError(Error);

#[derive(Debug, Fail)]
#[fail(display = "Empty Mutable Pack")]
pub struct EmptyMutablePack();

impl Fail for KeyError {
    fn backtrace(&self) -> Option<&Backtrace> {
        Some(self.0.backtrace())
    }

    fn cause(&self) -> Option<&dyn Fail> {
        Some(self.0.as_ref())
    }

    fn context<D>(self, context: D) -> Context<D>
    where
        D: fmt::Display + Send + Sync + 'static,
        Self: Sized,
    {
        self.0.context(context)
    }
}

impl fmt::Display for KeyError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "Key Error: {:?}", self.0)
    }
}

impl KeyError {
    pub fn new(err: Error) -> Self {
        KeyError(err)
    }
}
