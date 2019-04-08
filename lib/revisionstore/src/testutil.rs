// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use bytes::Bytes;

use types::Key;

use crate::datastore::Delta;

pub fn delta(data: &str, base: Option<Key>, key: Key) -> Delta {
    Delta {
        data: Bytes::from(data),
        base,
        key,
    }
}
