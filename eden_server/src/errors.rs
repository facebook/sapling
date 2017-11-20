// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::string::ParseError;

use blobrepo;
use mercurial_types;
use native_tls;
use secure_utils;

#[recursion_limit = "1024"]
error_chain! {
    links {
        Blobrepo(blobrepo::Error, blobrepo::ErrorKind);
        MercurialTypes(mercurial_types::Error, mercurial_types::ErrorKind);
        SecureUtils(secure_utils::Error, secure_utils::ErrorKind);
    }

    foreign_links {
        IoError(::std::io::Error);
        NativeTlsError(native_tls::Error);
        // This error is an Rust awkward hack
        // https://doc.rust-lang.org/std/string/enum.ParseError.html. Should never be encountered
        ParseError(ParseError);
    }
}
