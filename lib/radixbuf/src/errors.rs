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
        OffsetOverflow(offset: u64) {
            description("offset overflow")
            display("offset {} is out of range", offset)
        }
        AmbiguousPrefix {
            description("ambiguous prefix")
        }
        PrefixConflict(key_id1: KeyId, key_id2: KeyId) {
            description("key prefix conflict")
            display("{:?} cannot be a prefix of {:?}", key_id1, key_id2)
        }
        InvalidKeyId(key_id: KeyId) {
            description("invalid key id")
            display("{:?} cannot be resolved", key_id)
        }
        InvalidBase16(x: u8) {
            description("invalid base16 value")
            display("{} is not a base16 value", x)
        }
    }
}
