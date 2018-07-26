// Copyright 2004-present Facebook. All Rights Reserved.

use std::io;

#[recursion_limit = "1024"]
error_chain! {
    errors {
        NetstringDecode(msg: &'static str) {
            description("netstring decode error")
                display("{}", msg)
        }
    }

    foreign_links {
        Io(io::Error);
    }
}

impl From<Error> for io::Error {
    fn from(err: Error) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, format!("{}", err))
    }
}
