// Copyright 2017 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use key::KeyId;

error_chain! {
    foreign_links {
        Io(::std::io::Error);
    }

    errors {
        InvalidKeyId(key_id: KeyId) {
            description("invalid key id")
            display("{:?} cannot be resolved", key_id)
        }
    }
}
