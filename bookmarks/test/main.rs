// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Tests run against all bookmarks implementations.

#![deny(warnings)]

extern crate futures;
extern crate tempdir;
extern crate tokio_core;

extern crate bookmarks;
extern crate db;
extern crate dbbookmarks;
extern crate filebookmarks;
extern crate membookmarks;
extern crate mercurial_types;
extern crate mercurial_types_mocks;
extern crate storage_types;

use std::cell::RefCell;
use std::rc::Rc;

use futures::Stream;
use tempdir::TempDir;
use tokio_core::reactor::Core;

use bookmarks::BookmarksMut;
use dbbookmarks::DbBookmarks;
use filebookmarks::FileBookmarks;
use membookmarks::MemBookmarks;
use mercurial_types::NodeHash;
use mercurial_types_mocks::nodehash;
use storage_types::Version;

fn basic<B>(bookmarks: B, core: &mut Core)
where
    B: BookmarksMut<Value = NodeHash>,
{
    let foo = b"foo";
    let one = nodehash::ONES_HASH;
    let two = nodehash::TWOS_HASH;
    let three = nodehash::THREES_HASH;

    assert_eq!(core.run(bookmarks.get(&foo)).unwrap(), None);

    let absent = Version::absent();
    let foo_v1 = core.run(bookmarks.set(&foo, &one, &absent))
        .unwrap()
        .unwrap();
    assert_eq!(
        core.run(bookmarks.get(&foo)).unwrap(),
        Some((one.clone(), foo_v1))
    );

    let foo_v2 = core.run(bookmarks.set(&foo, &two, &foo_v1))
        .unwrap()
        .unwrap();

    // Should fail due to version mismatch.
    assert_eq!(
        core.run(bookmarks.set(&foo, &three, &foo_v1)).unwrap(),
        None
    );

    assert_eq!(
        core.run(bookmarks.delete(&foo, &foo_v2)).unwrap().unwrap(),
        absent
    );
    assert_eq!(core.run(bookmarks.get(&foo)).unwrap(), None);

    // Even though bookmark doesn't exist, this should fail with a version mismatch.
    assert_eq!(core.run(bookmarks.delete(&foo, &foo_v2)).unwrap(), None);

    // Deleting it with the absent version should work.
    assert_eq!(
        core.run(bookmarks.delete(&foo, &absent)).unwrap().unwrap(),
        absent
    );
}

fn list<B>(bookmarks: B, core: &mut Core)
where
    B: BookmarksMut<Value = NodeHash>,
{
    let one = b"1";
    let two = b"2";
    let three = b"3";
    let hash = nodehash::ONES_HASH;

    let _ = core.run(bookmarks.create(&one, &hash)).unwrap().unwrap();
    let _ = core.run(bookmarks.create(&two, &hash)).unwrap().unwrap();
    let _ = core.run(bookmarks.create(&three, &hash)).unwrap().unwrap();

    let mut result = core.run(bookmarks.keys().collect()).unwrap();
    result.sort();

    let expected = vec![one, two, three];
    assert_eq!(result, expected);
}

fn persistence<F, B>(mut new_bookmarks: F, core: Rc<RefCell<Core>>)
where
    F: FnMut() -> B,
    B: BookmarksMut<Value = NodeHash>,
{
    let foo = b"foo";
    let bar = nodehash::ONES_HASH;

    let version = {
        let bookmarks = new_bookmarks();
        core.borrow_mut()
            .run(bookmarks.create(&foo, &bar))
            .unwrap()
            .unwrap()
    };

    let bookmarks = new_bookmarks();
    assert_eq!(
        core.borrow_mut().run(bookmarks.get(&foo)).unwrap(),
        Some((bar, version))
    );
}

macro_rules! bookmarks_test_impl {
    ($mod_name: ident => {
        state: $state: expr,
        new: $new_cb: expr,
        persistent: $persistent: expr,
    }) => {
        mod $mod_name {
            use super::*;

            #[test]
            fn test_basic() {
                let mut core = Core::new().unwrap();
                let state = $state;
                let bookmarks = $new_cb(&state, &mut core);
                basic(bookmarks, &mut core);
            }

            #[test]
            fn test_list() {
                let mut core = Core::new().unwrap();
                let state = $state;
                let bookmarks = $new_cb(&state, &mut core);
                list(bookmarks, &mut core);
            }

            #[test]
            fn test_persistence() {
                // Not all bookmark implementations support persistence.
                if $persistent {
                    let core = Rc::new(RefCell::new(Core::new().unwrap()));
                    let state = $state;
                    let new_bookmarks = {
                        let core = Rc::clone(&core);
                        move || {
                            $new_cb(&state, &mut *core.borrow_mut())
                        }
                    };
                    persistence(new_bookmarks, core);
                }
            }
        }
    }
}

bookmarks_test_impl! {
    membookmarks_test => {
        state: (),
        new: |_, _| MemBookmarks::new(),
        persistent: false,
    }
}

bookmarks_test_impl! {
    filebookmarks_test => {
        state: TempDir::new("filebookmarks_test").unwrap(),
        new: |dir: &TempDir, _| FileBookmarks::open(dir.as_ref()).unwrap(),
        persistent: true,
    }
}

bookmarks_test_impl! {
    dbbookmarks_test => {
        state: dbbookmarks::init_test_db(),
        new: |params: &db::ConnectionParams, core: &mut Core| {
            let params = params.clone();
            let remote = core.remote();
            core.run(DbBookmarks::new_async(params, &remote)).unwrap()
        },
        persistent: true,
    }
}
