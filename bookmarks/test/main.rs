// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Tests run against all bookmarks implementations.

#![deny(warnings)]

extern crate futures;
extern crate tempdir;

extern crate bookmarks;
extern crate filebookmarks;
extern crate membookmarks;
extern crate storage_types;

use futures::{Future, Stream};
use tempdir::TempDir;

use bookmarks::BookmarksMut;
use filebookmarks::FileBookmarks;
use membookmarks::MemBookmarks;
use storage_types::Version;

fn basic<B>(bookmarks: B)
where
    B: BookmarksMut<Value = String>,
{
    let foo = "foo".to_string();
    let one = "1".to_string();
    let two = "2".to_string();
    let three = "3".to_string();

    assert_eq!(bookmarks.get(&foo).wait().unwrap(), None);

    let absent = Version::absent();
    let foo_v1 = bookmarks.set(&foo, &one, &absent).wait().unwrap().unwrap();
    assert_eq!(
        bookmarks.get(&foo).wait().unwrap(),
        Some((one.clone(), foo_v1))
    );

    let foo_v2 = bookmarks.set(&foo, &two, &foo_v1).wait().unwrap().unwrap();

    // Should fail due to version mismatch.
    assert_eq!(bookmarks.set(&foo, &three, &foo_v1).wait().unwrap(), None);

    assert_eq!(
        bookmarks.delete(&foo, &foo_v2).wait().unwrap().unwrap(),
        absent
    );
    assert_eq!(bookmarks.get(&foo).wait().unwrap(), None);

    // Even though bookmark doesn't exist, this should fail with a version mismatch.
    assert_eq!(bookmarks.delete(&foo, &foo_v2).wait().unwrap(), None);

    // Deleting it with the absent version should work.
    assert_eq!(
        bookmarks.delete(&foo, &absent).wait().unwrap().unwrap(),
        absent
    );
}

fn list<B>(bookmarks: B)
where
    B: BookmarksMut<Value = String>,
{
    let one = b"1";
    let two = b"2";
    let three = b"3";

    let _ = bookmarks
        .create(&one, &"foo".to_string())
        .wait()
        .unwrap()
        .unwrap();
    let _ = bookmarks
        .create(&two, &"bar".to_string())
        .wait()
        .unwrap()
        .unwrap();
    let _ = bookmarks
        .create(&three, &"baz".to_string())
        .wait()
        .unwrap()
        .unwrap();

    let mut result = bookmarks.keys().collect().wait().unwrap();
    result.sort();

    let expected = vec![one, two, three];
    assert_eq!(result, expected);
}

fn persistence<F, B>(mut new_bookmarks: F)
where
    F: FnMut() -> B,
    B: BookmarksMut<Value = String>,
{
    let foo = "foo".to_string();
    let bar = "bar".to_string();

    let version = {
        let bookmarks = new_bookmarks();
        bookmarks.create(&foo, &bar).wait().unwrap().unwrap()
    };

    let bookmarks = new_bookmarks();
    assert_eq!(bookmarks.get(&foo).wait().unwrap(), Some((bar, version)));
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
                let state = $state;
                basic($new_cb(&state));
            }

            #[test]
            fn test_list() {
                let state = $state;
                list($new_cb(&state));
            }

            #[test]
            fn test_persistence() {
                // Not all bookmark implementations support persistence.
                if $persistent {
                    let state = $state;
                    persistence(|| $new_cb(&state));
                }
            }
        }
    }
}

bookmarks_test_impl! {
    membookmarks_test => {
        state: (),
        new: |_| MemBookmarks::new(),
        persistent: false,
    }
}

bookmarks_test_impl! {
    filebookmarks_test => {
        state: TempDir::new("filebookmarks_test").unwrap(),
        new: |dir| FileBookmarks::open(&dir).unwrap(),
        persistent: true,
    }
}
