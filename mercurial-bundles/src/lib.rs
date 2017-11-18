// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate ascii;
#[macro_use]
#[cfg(test)]
extern crate assert_matches;
extern crate byteorder;
extern crate bytes;
#[macro_use]
extern crate error_chain;
#[macro_use]
extern crate futures;
#[macro_use]
extern crate lazy_static;
#[cfg(test)]
#[macro_use]
extern crate quickcheck;
#[cfg(test)]
extern crate rand;
#[macro_use]
extern crate slog;
#[cfg(test)]
extern crate slog_term;
#[cfg(test)]
extern crate tokio_core;
extern crate tokio_io;
extern crate tokio_proto;
extern crate url;

extern crate async_compression;
extern crate futures_ext;
extern crate mercurial_types;
#[cfg(test)]
extern crate partial_io;

pub mod bundle2;
pub mod bundle2_encode;
pub mod changegroup;
mod chunk;
mod delta;
pub mod parts;
pub mod part_encode;
mod part_header;
mod part_inner;
mod part_outer;
#[cfg(test)]
mod quickcheck_types;
mod stream_start;
mod types;

#[cfg(test)]
mod test;

mod errors;
pub use errors::*;
mod utils;

pub use bundle2_encode::Bundle2EncodeBuilder;
pub use part_header::PartHeader;
pub use part_inner::InnerPart;
pub use types::StreamHeader;

#[derive(Debug, Eq, PartialEq)]
pub enum Bundle2Item {
    Start(StreamHeader),
    Header(PartHeader),
    Inner(InnerPart),
}

impl Bundle2Item {
    pub fn is_start(&self) -> bool {
        match self {
            &Bundle2Item::Start(_) => true,
            _ => false,
        }
    }

    pub fn is_inner(&self) -> bool {
        match self {
            &Bundle2Item::Inner(_) => true,
            _ => false,
        }
    }

    #[cfg(test)]
    pub(crate) fn unwrap_start(self) -> StreamHeader {
        match self {
            Bundle2Item::Start(stream_header) => stream_header,
            other => panic!("expected item to be Start, was {:?}", other),
        }
    }

    #[cfg(test)]
    pub(crate) fn unwrap_inner(self) -> InnerPart {
        match self {
            Bundle2Item::Inner(inner_part) => inner_part,
            other => panic!("expected item to be Inner, was {:?}", other),
        }
    }
}
