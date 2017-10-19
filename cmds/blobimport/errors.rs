// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

error_chain! {
    links {
        Blobrepo(::blobrepo::Error, ::blobrepo::ErrorKind);
        Mercurial(::mercurial::Error, ::mercurial::ErrorKind);
        Rocksblob(::rocksblob::Error, ::rocksblob::ErrorKind);
        FileHeads(::fileheads::Error, ::fileheads::ErrorKind);
        Fileblob(::fileblob::Error, ::fileblob::ErrorKind);
        Manifold(::manifoldblob::Error, ::manifoldblob::ErrorKind);
    }
    foreign_links {
        Io(::std::io::Error);
    }
}

impl_kv_error!(Error);
