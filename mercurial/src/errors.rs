// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::io;

use mercurial_types;
use stockbookmarks;

#[recursion_limit = "1024"]
error_chain! {
    errors {
        Bundle2Decode(msg: String) {
            description("bundle2 decode error")
            display("{}", msg)
        }
        Revlog(msg: String) {
            description("revlog error")
            display("{}", msg)
        }
        Repo(msg: String) {
            description("repo error")
            display("{}", msg)
        }
        UnknownReq(req: String) {
            description("unknown repo requirement")
            display("Unknown requirement \"{}\"", req)
        }
    }

    links {
        MercurialTypes(mercurial_types::Error, mercurial_types::ErrorKind);
        StockBookmarks(stockbookmarks::Error, stockbookmarks::ErrorKind);
    }

    foreign_links {
        Io(::std::io::Error);
        Utf8(::std::str::Utf8Error);
        Utf8String(::std::string::FromUtf8Error);
    }
}

impl From<Error> for String {
    fn from(err: Error) -> String {
        format!("{}", err)
    }
}

impl From<Error> for io::Error {
    fn from(err: Error) -> io::Error {
        io::Error::new(io::ErrorKind::Other, format!("{}", err))
    }
}

impl From<ErrorKind> for io::Error {
    fn from(err: ErrorKind) -> io::Error {
        io::Error::new(io::ErrorKind::Other, format!("{}", err))
    }
}
