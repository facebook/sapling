// Copyright 2004-present Facebook. All Rights Reserved.

//! Tokio-based implementation of netstrings
//!
//! [Netstring](http://cr.yp.to/proto/netstrings.txt) is an extremely simple mechanism for
//! delimiting messages in a stream.
//!
//! Each message has the form "7:message," where the initial decimal number is the size of the
//! payload, followed by a ':', then the payload, and a terminating ','. There is no error
//! checking or correction other than the requirement that the message be followed by a comma.

extern crate bytes;
#[macro_use]
extern crate failure_ext as failure;
#[cfg(test)]
#[macro_use]
extern crate quickcheck;
extern crate tokio_io;

mod errors {
    pub use failure::{Error, Result};

    #[derive(Clone, Debug, Fail)]
    pub enum ErrorKind {
        #[fail(display = "{}", _0)] NetstringDecode(&'static str),
    }
}
pub use errors::*;

mod decode;
mod encode;

pub use decode::NetstringDecoder;
pub use encode::NetstringEncoder;
