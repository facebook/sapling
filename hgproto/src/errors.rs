// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::io;

use mercurial;
use mercurial_bundles;

#[recursion_limit = "1024"]
error_chain! {
    errors {
        Unimplemented(op: String) {
            description("unimplemented operation")
            display("unimplemented operation '{}'", op)
        }
        CommandParse(buf: Vec<u8>) {
            description("command parse failed")
            display("command parse failed for \"{}\"", String::from_utf8_lossy(&buf))
        }
        BatchInvalid(bs: Vec<u8>) {
            description("malformed batch command")
            display("malformed batch command '{}'", String::from_utf8_lossy(&bs))
        }
        BatchEscape(ch: u8) {
            description("unknown escape character in batch command")
            display("unknown escape character in batch command '{}'", ch)
        }
    }

    links {
        Mercurial(mercurial::Error, mercurial::ErrorKind);
        MercurialBundles(mercurial_bundles::Error, mercurial_bundles::ErrorKind);
    }

    foreign_links {
        Fmt(::std::fmt::Error);
        Io(::std::io::Error);
        Utf8(::std::str::Utf8Error);
        SendError(::futures::sync::mpsc::SendError<Vec<u8>>);
    }
}

impl From<Error> for io::Error {
    fn from(e: Error) -> Self {
        io::Error::new(io::ErrorKind::Other, format!("Error: {}", e))
    }
}
