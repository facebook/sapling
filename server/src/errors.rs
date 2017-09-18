// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use bytes::Bytes;

#[recursion_limit = "1024"]
error_chain! {
    links {
        Blobrepo(::blobrepo::Error, ::blobrepo::ErrorKind);
        HgProto(::hgproto::Error, ::hgproto::ErrorKind);
        Mercurial(::mercurial::Error, ::mercurial::ErrorKind);
    }

    foreign_links {
        Fmt(::std::fmt::Error);
        Io(::std::io::Error);
        SendError(::futures::sync::mpsc::SendError<Bytes>);
        Utf8(::std::str::Utf8Error);
    }
}
