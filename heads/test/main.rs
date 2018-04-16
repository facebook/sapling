// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Tests run against all heads implementations.

#![deny(warnings)]

extern crate futures;
extern crate tempdir;

extern crate fileheads;
extern crate heads;
extern crate memheads;
extern crate mercurial_types;
extern crate mercurial_types_mocks;

use futures::{Future, Stream};
use tempdir::TempDir;

use fileheads::FileHeads;
use heads::Heads;
use memheads::MemHeads;
use mercurial_types::DNodeHash;

fn basic<H: Heads>(heads: H) {
    let empty: Vec<DNodeHash> = Vec::new();
    assert_eq!(heads.heads().collect().wait().unwrap(), empty);

    let foo = mercurial_types_mocks::nodehash::ONES_HASH;
    let bar = mercurial_types_mocks::nodehash::TWOS_HASH;
    let baz = mercurial_types_mocks::nodehash::THREES_HASH;

    assert!(!heads.is_head(&foo).wait().unwrap());
    assert!(!heads.is_head(&bar).wait().unwrap());
    assert!(!heads.is_head(&baz).wait().unwrap());

    heads.add(&foo).wait().unwrap();
    heads.add(&bar).wait().unwrap();

    assert!(heads.is_head(&foo).wait().unwrap());
    assert!(heads.is_head(&bar).wait().unwrap());
    assert!(!heads.is_head(&baz).wait().unwrap());

    let mut result = heads.heads().collect().wait().unwrap();
    result.sort();

    assert_eq!(result, vec![foo.clone(), bar.clone()]);

    heads.remove(&foo).wait().unwrap();
    heads.remove(&bar).wait().unwrap();
    heads.remove(&baz).wait().unwrap(); // Removing non-existent head should not panic.

    assert_eq!(heads.heads().collect().wait().unwrap(), empty);
}

fn persistence<F, H>(mut new_heads: F)
where
    F: FnMut() -> H,
    H: Heads,
{
    let foo = mercurial_types_mocks::nodehash::ONES_HASH;
    let bar = mercurial_types_mocks::nodehash::TWOS_HASH;

    {
        let heads = new_heads();
        heads.add(&foo).wait().unwrap();
        heads.add(&bar).wait().unwrap();
    }

    let heads = new_heads();
    let mut result = heads.heads().collect().wait().unwrap();
    result.sort();
    assert_eq!(result, vec![foo.clone(), bar.clone()]);
}

fn save_node_hash<H: Heads>(heads: H) {
    let head = mercurial_types_mocks::nodehash::AS_HASH;
    heads.add(&head).wait().unwrap();
    let mut result = heads.heads().collect().wait().unwrap();
    result.sort();
    assert_eq!(result, vec![head]);
}

macro_rules! heads_test_impl {
    ($mod_name: ident => {
        state: $state: expr,
        new: $new_cb: expr,
        persistent: $persistent: expr,
    }) => {
        mod $mod_name {
            use super::*;

            #[test]
            fn test_basic() {
                let state = $state;
                basic($new_cb(&state));
            }

            #[test]
            fn test_save_node_hash() {
                let state = $state;
                save_node_hash($new_cb(&state));
            }

            #[test]
            fn test_persistence() {
                // Not all heads implementations support persistence.
                if $persistent {
                    let state = $state;
                    persistence(|| $new_cb(&state));
                }
            }
        }
    }
}

heads_test_impl! {
    memheads_test => {
        state: (),
        new: |_| MemHeads::new(),
        persistent: false,
    }
}

heads_test_impl! {
    fileheads_test => {
        state: TempDir::new("fileheads_test").unwrap(),
        new: |dir| FileHeads::open(&dir).unwrap(),
        persistent: true,
    }
}
