// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobrepo;
use mercurial_types;
use std::string::ParseError;

#[recursion_limit = "1024"]
error_chain! {
    links {
        MercurialTypes(mercurial_types::Error, mercurial_types::ErrorKind);
        Blobrepo(blobrepo::Error, blobrepo::ErrorKind);
    }

    foreign_links {
        // This error is an Rust awkward hack
        // https://doc.rust-lang.org/std/string/enum.ParseError.html. Should never be encountered
        ParseError(ParseError);
    }
}
