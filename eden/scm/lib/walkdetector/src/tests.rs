/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::time::Instant;

use anyhow::Result;
use types::RepoPathBuf;

use crate::Detector;
use crate::Walk;
use crate::walk_node::WalkNode;

fn p(p: impl AsRef<str>) -> RepoPathBuf {
    p.as_ref().to_string().try_into().unwrap()
}

#[test]
fn test_walk_big_dir() -> Result<()> {
    let detector = Detector::new();
    detector.set_min_dir_walk_threshold(2);

    assert_eq!(detector.walks().len(), 0);

    let epoch = Instant::now();

    detector.file_read(epoch, p("dir/a"));
    detector.file_read(epoch, p("dir/a"));

    assert_eq!(detector.walks().len(), 0);

    detector.file_read(epoch, p("dir/b"));

    assert_eq!(detector.walks(), vec![(p("dir"), 0)]);

    detector.file_read(epoch, p("dir/c"));
    detector.file_read(epoch, p("dir/d"));
    detector.file_read(epoch, p("dir/e"));

    assert_eq!(detector.walks(), vec![(p("dir"), 0)]);

    Ok(())
}

#[test]
fn test_bfs_walk() -> Result<()> {
    let detector = Detector::new();
    detector.set_min_dir_walk_threshold(2);

    let epoch = Instant::now();

    detector.file_read(epoch, p("root/dir1/a"));
    detector.file_read(epoch, p("root/dir1/b"));

    assert_eq!(detector.walks(), vec![(p("root/dir1"), 0)]);

    detector.file_read(epoch, p("root/dir2/a"));
    detector.file_read(epoch, p("root/dir2/b"));

    // Raised walk up to parent directory with depth=1.
    assert_eq!(detector.walks(), vec![(p("root"), 1)]);

    // Now walk proceeds to the next level.

    detector.file_read(epoch, p("root/dir1/dir1_1/a"));
    detector.file_read(epoch, p("root/dir1/dir1_1/b"));

    // Nothing combined yet.
    assert_eq!(
        detector.walks(),
        vec![(p("root"), 1), (p("root/dir1/dir1_1"), 0)]
    );

    detector.file_read(epoch, p("root/dir1/dir1_2/a"));
    detector.file_read(epoch, p("root/dir1/dir1_2/b"));

    // Now we get a second walk for root/dir1
    assert_eq!(detector.walks(), vec![(p("root"), 1), (p("root/dir1"), 1)]);

    // More reads in dir2 doesn't combine the walks
    detector.file_read(epoch, p("root/dir2/c"));
    detector.file_read(epoch, p("root/dir2/d"));
    assert_eq!(detector.walks(), vec![(p("root"), 1), (p("root/dir1"), 1)]);

    // Walking further in root/dir2 will combine up to root.
    detector.file_read(epoch, p("root/dir2/dir2_1/a"));
    detector.file_read(epoch, p("root/dir2/dir2_1/b"));
    detector.file_read(epoch, p("root/dir2/dir2_2/a"));
    detector.file_read(epoch, p("root/dir2/dir2_2/b"));
    assert_eq!(detector.walks(), vec![(p("root"), 2)]);

    Ok(())
}

#[test]
fn test_walk_node_insert() {
    let mut node = WalkNode::default();

    let epoch = Instant::now();

    let foo_walk = Walk {
        depth: 1,
        last_access: epoch,
    };
    node.insert(&p("foo"), foo_walk);
    // Can re-insert.
    node.insert(&p("foo"), foo_walk);
    assert_eq!(node.list(), vec![(p("foo"), foo_walk)]);

    // Don't insert since it is fully contained by "foo" walk.
    node.insert(
        &p("foo/bar"),
        Walk {
            depth: 0,
            last_access: epoch,
        },
    );
    assert_eq!(node.list(), vec![(p("foo"), foo_walk)]);

    let baz_walk = Walk {
        depth: 2,
        last_access: epoch,
    };
    node.insert(&p("foo/bar/baz"), baz_walk);
    assert_eq!(
        node.list(),
        vec![(p("foo"), foo_walk), (p("foo/bar/baz"), baz_walk)]
    );

    let root_walk = Walk {
        depth: 0,
        last_access: epoch,
    };
    node.insert(&p(""), root_walk);
    assert_eq!(
        node.list(),
        vec![
            (p(""), root_walk),
            (p("foo"), foo_walk),
            (p("foo/bar/baz"), baz_walk)
        ]
    );

    // depth=1 doesn't contain any descendant walks - don't clear anything out.
    let root_walk = Walk {
        depth: 1,
        last_access: epoch,
    };
    node.insert(&p(""), root_walk);
    assert_eq!(
        node.list(),
        vec![
            (p(""), root_walk),
            (p("foo"), foo_walk),
            (p("foo/bar/baz"), baz_walk)
        ]
    );

    // depth=2 contains the "foo" walk - clear "foo" out.
    let root_walk = Walk {
        depth: 2,
        last_access: epoch,
    };
    node.insert(&p(""), root_walk);
    assert_eq!(
        node.list(),
        vec![(p(""), root_walk), (p("foo/bar/baz"), baz_walk)]
    );

    // Contains the "foo/bar/baz" walk.
    let root_walk = Walk {
        depth: 5,
        last_access: epoch,
    };
    node.insert(&p(""), root_walk);
    assert_eq!(node.list(), vec![(p(""), root_walk),]);
}

#[test]
fn test_walk_node_get() {
    let mut node = WalkNode::default();

    assert!(node.get(&p("")).is_none());
    assert!(node.get(&p("foo")).is_none());
    assert!(node.get(&p("foo/bar")).is_none());

    let epoch = Instant::now();

    let mut foo_walk = Walk {
        depth: 1,
        last_access: epoch,
    };
    node.insert(&p("foo"), foo_walk);

    assert!(node.get(&p("")).is_none());
    assert_eq!(node.get(&p("foo")), Some(&mut foo_walk));
    assert!(node.get(&p("foo/bar")).is_none());

    let mut foo_bar_walk = Walk {
        depth: 2,
        last_access: epoch,
    };
    node.insert(&p("foo/bar"), foo_bar_walk);

    assert!(node.get(&p("")).is_none());
    assert_eq!(node.get(&p("foo")), Some(&mut foo_walk));
    assert_eq!(node.get(&p("foo/bar")), Some(&mut foo_bar_walk));

    let mut root_walk = Walk {
        depth: 0,
        last_access: epoch,
    };
    node.insert(&p(""), root_walk);

    assert_eq!(node.get(&p("")), Some(&mut root_walk));
    assert_eq!(node.get(&p("foo")), Some(&mut foo_walk));
    assert_eq!(node.get(&p("foo/bar")), Some(&mut foo_bar_walk));
}

#[test]
fn test_walk_get_containing() {
    let mut node = WalkNode::default();

    let dir = p("foo/bar/baz");

    assert!(node.get_containing(&dir).is_none());

    let epoch = Instant::now();

    let mut walk = Walk {
        depth: 0,
        last_access: epoch,
    };
    node.insert(&p("foo/bar"), walk);

    // Still not containing due to depth.
    assert!(node.get_containing(&dir).is_none());

    walk.depth = 1;
    node.insert(&p("foo/bar"), walk);

    assert_eq!(node.get_containing(&dir), Some(&mut walk));
}
