// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::io;

use ascii::AsciiString;

use part_header::PartHeader;

#[recursion_limit = "1024"]
error_chain! {
    errors {
        Bundle2Decode(msg: String) {
            description("bundle2 decode error")
                display("{}", msg)
        }
        Cg2Decode(msg: String) {
            description("changegroup2 decode error")
            display("changegroup2 decode error: {}", msg)
        }
        Bundle2Encode(msg: String) {
            description("bundle2 encode error")
            display("{}", msg)
        }
        Bundle2Chunk(msg: String) {
            description("bundle2 chunk error")
            display("bundle2 chunk error: {}", msg)
        }
        BundleUnknownPart(header: PartHeader) {
            description("unknown bundle2 part type")
            display("unknown part type: {}", header.part_type())
        }
        BundleUnknownPartParams(part_type: AsciiString, params: Vec<String>) {
            description("unknown bundle2 part params")
            display("unknown params for bundle2 part '{}': {}", part_type, params.join(", "))
        }
        ListkeyGeneration {
            description("error while generating listkey part")
            display("error while generating listkey part")
        }
    }

    foreign_links {
        Io(io::Error);
    }
}

impl Error {
    pub fn is_app_error(&self) -> bool {
        match self {
            &Error(ErrorKind::BundleUnknownPart(_), _) |
            &Error(ErrorKind::BundleUnknownPartParams(..), _) => true,
            _ => false,
        }
    }
}

impl From<Error> for io::Error {
    fn from(err: Error) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, format!("{}", err))
    }
}

impl From<ErrorKind> for io::Error {
    fn from(err: ErrorKind) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, format!("{}", err))
    }
}
