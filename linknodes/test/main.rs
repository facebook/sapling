// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Tests run against all linknode implementations.

#![deny(warnings)]

#[macro_use]
extern crate assert_matches;
extern crate futures;
extern crate tempdir;

extern crate filelinknodes;
extern crate linknodes;
extern crate memlinknodes;
extern crate mercurial_types;
extern crate mercurial_types_mocks;

use futures::Future;
use tempdir::TempDir;

use filelinknodes::FileLinknodes;
use linknodes::{ErrorKind, Linknodes};
use memlinknodes::MemLinknodes;
use mercurial_types::MPath;
use mercurial_types_mocks::nodehash::*;

fn add_and_get<L: Linknodes>(linknodes: L) {
    let path = MPath::new("abc").unwrap();
    linknodes.add(&path, &NULL_HASH, &ONES_HASH).wait().unwrap();
    linknodes.add(&path, &AS_HASH, &TWOS_HASH).wait().unwrap();

    // This will error out because this combination already exists.
    assert_matches!(
        linknodes
            .add(&path, &NULL_HASH, &THREES_HASH)
            .wait()
            .unwrap_err()
            .kind(),
        &ErrorKind::AlreadyExists(ref p, ref h, ref old, ref new)
        if p == &path && *h == NULL_HASH && old.unwrap_or(ONES_HASH) == ONES_HASH &&
        *new == THREES_HASH
    );

    assert_eq!(linknodes.get(&path, &NULL_HASH).wait().unwrap(), ONES_HASH);
    assert_eq!(linknodes.get(&path, &AS_HASH).wait().unwrap(), TWOS_HASH);
}

fn not_found<L: Linknodes>(linknodes: L) {
    let path = MPath::new("abc").unwrap();
    assert_matches!(
        linknodes
            .get(&path, &NULL_HASH)
            .wait()
            .unwrap_err()
            .kind(),
        &ErrorKind::NotFound(ref p, ref h) if p == &path && *h == NULL_HASH
    );
}

fn persistence<F, L>(mut new_linknodes: F)
where
    F: FnMut() -> L,
    L: Linknodes,
{
    let path = MPath::new("abc").unwrap();
    {
        let linknodes = new_linknodes();
        linknodes.add(&path, &NULL_HASH, &ONES_HASH).wait().unwrap();
    }

    let linknodes = new_linknodes();
    assert_eq!(linknodes.get(&path, &NULL_HASH).wait().unwrap(), ONES_HASH);
}

macro_rules! linknodes_test_impl {
    ($mod_name: ident => {
        state: $state: expr,
        new: $new_cb: expr,
        persistent: $persistent: expr,
    }) => {
        mod $mod_name {
            use super::*;

            #[test]
            fn test_add_and_get() {
                let state = $state;
                add_and_get($new_cb(&state));
            }

            #[test]
            fn test_not_found() {
                let state = $state;
                not_found($new_cb(&state));
            }

            #[test]
            fn test_persistence() {
                // Not all linknode implementations support persistence. There doesn't seem to be
                // a neat way to not define a function based on a boolean, though, so this is
                // the best we can probably do.
                if $persistent {
                    let state = $state;
                    persistence(|| $new_cb(&state));
                }
            }
        }
    }
}

linknodes_test_impl! {
    memlinknodes_test => {
        state: (),
        new: |_| MemLinknodes::new(),
        persistent: false,
    }
}

linknodes_test_impl! {
    filelinknodes_test => {
        state: TempDir::new("filelinknodes_test").unwrap(),
        new: |dir| FileLinknodes::open(&dir).unwrap(),
        persistent: true,
    }
}
