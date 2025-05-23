/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::time::Duration;
use std::time::Instant;

use types::RepoPath;
use types::RepoPathBuf;

use crate::Detector;
use crate::Walk;
use crate::WalkType;
use crate::walk_node::WalkNode;

fn p(p: impl AsRef<str>) -> RepoPathBuf {
    p.as_ref().to_string().try_into().unwrap()
}

const TEST_WALK_THRESHOLD: usize = 2;

#[test]
fn test_walk_big_dir() {
    let detector = Detector::new();
    detector.set_walk_threshold(TEST_WALK_THRESHOLD);

    assert_eq!(detector.file_walks().len(), 0);

    detector.file_loaded(p("dir/a"));
    detector.file_loaded(p("dir/a"));

    assert_eq!(detector.file_walks().len(), 0);

    detector.file_loaded(p("dir/b"));

    assert_eq!(detector.file_walks(), vec![(p("dir"), 0)]);

    detector.file_loaded(p("dir/c"));
    detector.file_loaded(p("dir/d"));
    detector.file_loaded(p("dir/e"));

    assert_eq!(detector.file_walks(), vec![(p("dir"), 0)]);
}

#[test]
fn test_bfs_walk() {
    let detector = Detector::new();
    detector.set_walk_threshold(TEST_WALK_THRESHOLD);

    detector.file_loaded(p("root/dir1/a"));
    detector.file_loaded(p("root/dir1/b"));

    assert_eq!(detector.file_walks(), vec![(p("root/dir1"), 0)]);

    detector.file_loaded(p("root/dir2/a"));
    detector.file_loaded(p("root/dir2/b"));

    // Raised walk up to parent directory with depth=1.
    assert_eq!(detector.file_walks(), vec![(p("root"), 1)]);

    // Now walk proceeds to the next level.

    detector.file_loaded(p("root/dir1/dir1_1/a"));
    detector.file_loaded(p("root/dir1/dir1_1/b"));

    // Nothing combined yet.
    assert_eq!(
        detector.file_walks(),
        vec![(p("root"), 1), (p("root/dir1/dir1_1"), 0)]
    );

    detector.file_loaded(p("root/dir1/dir1_2/a"));
    detector.file_loaded(p("root/dir1/dir1_2/b"));

    // Now we get a second walk for root/dir1
    assert_eq!(
        detector.file_walks(),
        vec![(p("root"), 1), (p("root/dir1"), 1)]
    );

    // More reads in dir2 doesn't combine the walks
    detector.file_loaded(p("root/dir2/c"));
    detector.file_loaded(p("root/dir2/d"));
    assert_eq!(
        detector.file_walks(),
        vec![(p("root"), 1), (p("root/dir1"), 1)]
    );

    // Walking further in root/dir2 will combine up to root.
    detector.file_loaded(p("root/dir2/dir2_1/a"));
    detector.file_loaded(p("root/dir2/dir2_1/b"));
    detector.file_loaded(p("root/dir2/dir2_2/a"));
    detector.file_loaded(p("root/dir2/dir2_2/b"));
    assert_eq!(detector.file_walks(), vec![(p("root"), 2)]);

    // Walk boundary can advance after we see depth+1 access that bubbles up to 2
    // different children of root.
    detector.file_loaded(p("root/dir2/dir2_1/dir2_1_1/a"));
    detector.file_loaded(p("root/dir2/dir2_1/dir2_1_1/b"));

    // So far only one advancement - doesn't expand root walk yet.
    assert_eq!(
        detector.file_walks(),
        vec![(p("root"), 2), (p("root/dir2/dir2_1/dir2_1_1"), 0)]
    );

    // Doesn't bubble up since advancement is still only under a single child "dir2".
    detector.file_loaded(p("root/dir2/dir2_2/dir2_2_1/a"));
    detector.file_loaded(p("root/dir2/dir2_2/dir2_2_1/b"));
    assert_eq!(
        detector.file_walks(),
        vec![(p("root"), 2), (p("root/dir2"), 2)]
    );

    // Now we also see a depth=3 access under "dir1" - expand depth.
    detector.file_loaded(p("root/dir1/dir1_1/dir1_1_1/a"));
    detector.file_loaded(p("root/dir1/dir1_1/dir1_1_1/b"));
    assert_eq!(detector.file_walks(), vec![(p("root"), 3)]);
}

#[test]
fn test_advanced_remainder() {
    let detector = Detector::new();
    detector.set_walk_threshold(TEST_WALK_THRESHOLD);

    detector.file_loaded(p("root/dir1/a"));
    detector.file_loaded(p("root/dir1/b"));
    detector.file_loaded(p("root/dir2/a"));
    detector.file_loaded(p("root/dir2/b"));
    detector.file_loaded(p("root/dir1/dir1_1/a"));
    detector.file_loaded(p("root/dir1/dir1_1/b"));

    detector.file_loaded(p("root/dir2/dir2_1/a"));
    detector.file_loaded(p("root/dir2/dir2_1/b"));
    assert_eq!(detector.file_walks(), vec![(p("root"), 2)]);

    // This marks "root/dir1" as "advanced" child.
    detector.file_loaded(p("root/dir1/dir1_1/dir1_1_1/a"));
    detector.file_loaded(p("root/dir1/dir1_1/dir1_1_1/b"));
    assert_eq!(
        detector.file_walks(),
        vec![(p("root"), 2), (p("root/dir1/dir1_1/dir1_1_1"), 0)]
    );

    // This marks "root/dir2" as "advanced" child, but the
    // root/dir2/dir2_1 walk extends deeper than the advanced walk -
    // don't remove it.
    detector.file_loaded(p("root/dir2/dir2_1/dir2_1_1/dir2_1_1_1/a"));
    detector.file_loaded(p("root/dir2/dir2_1/dir2_1_1/dir2_1_1_1/b"));
    detector.file_loaded(p("root/dir2/dir2_1/dir2_1_1/dir2_1_1_2/a"));
    detector.file_loaded(p("root/dir2/dir2_1/dir2_1_1/dir2_1_1_2/b"));

    assert_eq!(
        detector.file_walks(),
        vec![(p("root"), 3), (p("root/dir2/dir2_1/dir2_1_1"), 1)]
    );
}

#[test]
fn test_walk_node_insert() {
    let mut node = WalkNode::default();

    node.insert_walk(WalkType::File, &p("foo"), Walk::new(1), TEST_WALK_THRESHOLD);
    // Can re-insert.
    node.insert_walk(WalkType::File, &p("foo"), Walk::new(1), TEST_WALK_THRESHOLD);
    assert_eq!(node.list_walks(WalkType::File), vec![(p("foo"), 1)]);

    // Don't insert since it is fully contained by "foo" walk.
    node.insert_walk(
        WalkType::File,
        &p("foo/bar"),
        Walk::new(0),
        TEST_WALK_THRESHOLD,
    );
    assert_eq!(node.list_walks(WalkType::File), vec![(p("foo"), 1)]);

    let baz_walk = Walk::new(2);
    node.insert_walk(
        WalkType::File,
        &p("foo/bar/baz"),
        baz_walk,
        TEST_WALK_THRESHOLD,
    );
    assert_eq!(
        node.list_walks(WalkType::File),
        vec![(p("foo"), 1), (p("foo/bar/baz"), 2)]
    );

    let root_walk = Walk::new(0);
    node.insert_walk(WalkType::File, &p(""), root_walk, TEST_WALK_THRESHOLD);
    assert_eq!(
        node.list_walks(WalkType::File),
        vec![(p(""), 0), (p("foo"), 1), (p("foo/bar/baz"), 2)]
    );

    // depth=1 doesn't contain any descendant walks - don't clear anything out.
    let root_walk = Walk::new(1);
    node.insert_walk(WalkType::File, &p(""), root_walk, TEST_WALK_THRESHOLD);
    assert_eq!(
        node.list_walks(WalkType::File),
        vec![(p(""), 1), (p("foo"), 1), (p("foo/bar/baz"), 2)]
    );

    // depth=2 contains the "foo" walk - clear "foo" out.
    let root_walk = Walk::new(2);
    node.insert_walk(WalkType::File, &p(""), root_walk, TEST_WALK_THRESHOLD);
    assert_eq!(
        node.list_walks(WalkType::File),
        vec![(p(""), 2), (p("foo/bar/baz"), 2)]
    );

    // Contains the "foo/bar/baz" walk.
    let root_walk = Walk::new(5);
    node.insert_walk(WalkType::File, &p(""), root_walk, TEST_WALK_THRESHOLD);
    assert_eq!(node.list_walks(WalkType::File), vec![(p(""), 5)]);
}

#[test]
fn test_walk_node_get() {
    let mut node = WalkNode::default();

    let get_depth = |node: &mut WalkNode, path: &RepoPath| -> Option<usize> {
        node.get_node(path)
            .and_then(|node| node.file_walk.as_ref().map(|walk| walk.depth))
    };

    assert!(get_depth(&mut node, &p("")).is_none());
    assert!(get_depth(&mut node, &p("foo")).is_none());
    assert!(get_depth(&mut node, &p("foo/bar")).is_none());

    let foo_walk = Walk::new(1);
    node.insert_walk(WalkType::File, &p("foo"), foo_walk, TEST_WALK_THRESHOLD);

    assert!(get_depth(&mut node, &p("")).is_none());
    assert_eq!(get_depth(&mut node, &p("foo")), Some(1));
    assert!(get_depth(&mut node, &p("foo/bar")).is_none());

    let foo_bar_walk = Walk::new(2);
    node.insert_walk(
        WalkType::File,
        &p("foo/bar"),
        foo_bar_walk,
        TEST_WALK_THRESHOLD,
    );

    assert!(get_depth(&mut node, &p("")).is_none());
    assert_eq!(get_depth(&mut node, &p("foo")), Some(1));
    assert_eq!(get_depth(&mut node, &p("foo/bar")), Some(2));

    let root_walk = Walk::new(0);
    node.insert_walk(WalkType::File, &p(""), root_walk, TEST_WALK_THRESHOLD);

    assert_eq!(get_depth(&mut node, &p("")), Some(0));
    assert_eq!(get_depth(&mut node, &p("foo")), Some(1));
    assert_eq!(get_depth(&mut node, &p("foo/bar")), Some(2));
}

#[test]
fn test_walk_get_containing_node() {
    let mut node = WalkNode::default();

    let dir = p("foo/bar/baz");

    assert!(node.get_owning_node(WalkType::File, &dir).is_none());

    node.insert_walk(
        WalkType::File,
        &p("foo/bar"),
        Walk::new(0),
        TEST_WALK_THRESHOLD,
    );

    // Still not containing due to depth.
    assert!(node.get_owning_node(WalkType::File, &dir).is_none());

    node.insert_walk(
        WalkType::File,
        &p("foo/bar"),
        Walk::new(1),
        TEST_WALK_THRESHOLD,
    );

    let (containing_node, suffix) = node.get_owning_node(WalkType::File, &dir).unwrap();
    assert_eq!(
        containing_node
            .get_walk_for_type(WalkType::File)
            .unwrap()
            .depth,
        1
    );
    assert_eq!(suffix, p("baz"));
}

#[test]
fn test_dir_hints() {
    let detector = Detector::new();
    detector.set_walk_threshold(TEST_WALK_THRESHOLD);

    // Hint that "dir" has 0 files and 1 directory.
    detector.dir_loaded(p("dir"), 0, 1);

    // Hint that "dir/subdir" has 1 file and 0 directories.
    detector.dir_loaded(p("dir/subdir"), 1, 0);

    detector.file_loaded(p("dir/subdir/a"));

    // The walk bubbled straight up to "dir".
    assert_eq!(detector.file_walks(), vec![(p("dir"), 1)]);
}

#[test]
fn test_advance_while_advancing() {
    // Test that we can "advance" the walk depth twice in a row when
    // the descendant walks have depths greater than zero.
    let detector = Detector::new();
    detector.set_walk_threshold(TEST_WALK_THRESHOLD);

    // Walk at root/, depth=1.
    detector.file_loaded(p("root/dir1/a"));
    detector.file_loaded(p("root/dir1/b"));
    detector.file_loaded(p("root/dir2/a"));
    detector.file_loaded(p("root/dir2/b"));
    assert_eq!(detector.file_walks(), vec![(p("root"), 1)]);

    // Walk at root/dir1/dir1_1, depth=1.
    // Adds "dir1" advanced child to root/ walk.
    detector.file_loaded(p("root/dir1/dir1_1/dir1_1_1/a"));
    detector.file_loaded(p("root/dir1/dir1_1/dir1_1_1/b"));
    detector.file_loaded(p("root/dir1/dir1_1/dir1_1_2/a"));
    detector.file_loaded(p("root/dir1/dir1_1/dir1_1_2/b"));
    assert_eq!(
        detector.file_walks(),
        vec![(p("root"), 1), (p("root/dir1/dir1_1"), 1)]
    );

    // Walk at root/dir2/dir2_1, depth=1.
    // Adds "dir2" advanced child to root/ walk.
    detector.file_loaded(p("root/dir2/dir2_1/dir2_1_1/a"));
    detector.file_loaded(p("root/dir2/dir2_1/dir2_1_1/b"));
    detector.file_loaded(p("root/dir2/dir2_1/dir2_1_2/a"));
    detector.file_loaded(p("root/dir2/dir2_1/dir2_1_2/b"));

    assert_eq!(detector.file_walks(), vec![(p("root"), 3)]);
}

#[test]
fn test_retain_interesting_metadata() {
    let detector = Detector::new();
    detector.set_walk_threshold(TEST_WALK_THRESHOLD);

    // "interesting" metadata saying root/dir1 only has one directory
    detector.dir_loaded(p("root/dir1"), 2, 1);

    // Walk at root/, depth=1.
    detector.file_loaded(p("root/dir1/a"));
    detector.file_loaded(p("root/dir1/b"));
    detector.file_loaded(p("root/dir2/a"));
    detector.file_loaded(p("root/dir2/b"));
    assert_eq!(detector.file_walks(), vec![(p("root"), 1)]);

    // Walk at root/dir1/dir1_1, depth=1.
    // Test that we "remembered" root/dir1 metadata, so doot/dir1/dir1_1 is instantly promoted into walk on root/dir1.
    detector.file_loaded(p("root/dir1/dir1_1/a"));
    detector.file_loaded(p("root/dir1/dir1_1/b"));
    assert_eq!(
        detector.file_walks(),
        vec![(p("root"), 1), (p("root/dir1"), 1)]
    );
}

#[test]
fn test_merge_cousins() {
    let detector = Detector::new();
    detector.set_walk_threshold(TEST_WALK_THRESHOLD);

    detector.dir_loaded(p("root"), 0, 1);

    // Walk at root/, depth=1.
    detector.file_loaded(p("root/foo/dir1/a"));
    detector.file_loaded(p("root/foo/dir1/b"));
    detector.file_loaded(p("root/foo/dir2/a"));
    detector.file_loaded(p("root/foo/dir2/b"));
    assert_eq!(detector.file_walks(), vec![(p("root"), 2)]);

    detector.file_loaded(p("root/foo/dir1/dir1_1/a"));
    detector.file_loaded(p("root/foo/dir1/dir1_1/b"));
    detector.file_loaded(p("root/foo/dir2/dir2_1/a"));
    detector.file_loaded(p("root/foo/dir2/dir2_1/b"));
    assert_eq!(detector.file_walks(), vec![(p("root"), 3)]);
}

#[test]
fn test_gc() {
    let detector = Detector::new();
    detector.set_walk_threshold(TEST_WALK_THRESHOLD);
    detector.set_gc_interval(Duration::from_secs(1));
    detector.set_gc_timeout(Duration::from_secs(2));

    let mut epoch = Instant::now();
    detector.set_now(epoch);

    detector.file_loaded(p("dir1/a"));
    assert_eq!(detector.file_walks(), vec![]);

    epoch += Duration::from_secs(1);
    detector.set_now(epoch);

    // GC should run but not remove anything.
    detector.file_loaded(p("dir1/b"));
    assert_eq!(detector.file_walks(), vec![(p("dir1"), 0)]);

    epoch += Duration::from_secs(1);
    detector.set_now(epoch);

    // This should keep dir1 walk alive.
    detector.file_loaded(p("dir1/c"));
    detector.file_loaded(p("dir2/a"));
    detector.file_loaded(p("some/deep/dir/a"));
    assert_eq!(detector.file_walks(), vec![(p("dir1"), 0)]);

    epoch += Duration::from_secs(1);
    detector.set_now(epoch);

    detector.file_loaded(p("dir2/b"));
    assert_eq!(detector.file_walks(), vec![(p(""), 1)]);

    epoch += Duration::from_secs(1);
    detector.set_now(epoch);

    // GC should clear out some/deep/dir, so this should not result in walk.
    detector.file_loaded(p("some/deep/dir/b"));
    // This should update access time for root walk.
    detector.file_loaded(p("dir3/a"));
    assert_eq!(detector.file_walks(), vec![(p(""), 1)]);

    // Root walk still here since dir3/a refreshed access time.
    epoch += Duration::from_secs(1);
    detector.set_now(epoch);
    assert_eq!(detector.file_walks(), vec![(p(""), 1)]);

    // Everything is GC'd.
    epoch += Duration::from_secs(1);
    detector.set_now(epoch);
    assert_eq!(detector.file_walks(), vec![]);
}

#[test]
fn test_gc_stats() {
    let detector = Detector::new();
    detector.set_walk_threshold(TEST_WALK_THRESHOLD);

    // Don't run GC automatically.
    detector.set_gc_interval(Duration::from_secs(10));

    let mut epoch = Instant::now();
    detector.set_now(epoch);

    detector.file_loaded(p("dir1/a"));
    detector.file_loaded(p("dir1/b"));

    detector.file_loaded(p("dir2/a"));

    detector.file_loaded(p("dir3/dir4/a"));

    epoch += Duration::from_secs(2);
    detector.set_now(epoch);

    // Refresh access time on dir3.
    detector.file_loaded(p("dir3/dir4/b"));

    // Manually run GC to check stats.
    let (nodes_removed, nodes_remaining, walks_removed) = detector
        .inner
        .write()
        .node
        .gc(Duration::from_secs(1), epoch);

    // "dir1" and "dir2"
    assert_eq!(nodes_removed, 2);

    // root node and "dir3" and "dir4"
    assert_eq!(nodes_remaining, 3);

    // "dir1"
    assert_eq!(walks_removed, 1);
}

#[test]
fn test_dir_walk() {
    let detector = Detector::new();
    detector.set_walk_threshold(TEST_WALK_THRESHOLD);

    detector.dir_loaded(p(""), 2, 2);
    assert_eq!(detector.dir_walks(), vec![]);

    detector.dir_loaded(p("dir1"), 2, 2);
    detector.dir_loaded(p("dir2"), 2, 2);
    assert_eq!(detector.dir_walks(), vec![(p(""), 0)]);

    detector.dir_loaded(p("dir1/dir1_1"), 2, 2);
    detector.dir_loaded(p("dir1/dir1_2"), 2, 2);
    detector.dir_loaded(p("dir2/dir2_1"), 2, 2);
    detector.dir_loaded(p("dir2/dir2_2"), 2, 2);
    assert_eq!(detector.dir_walks(), vec![(p(""), 1)]);

    // Now we start seeing files walked.
    detector.file_loaded(p("a"));
    detector.file_loaded(p("b"));
    assert_eq!(detector.file_walks(), vec![(p(""), 0)]);
    // Directory walk still around since it is deeper than file walk.
    assert_eq!(detector.dir_walks(), vec![(p(""), 1)]);

    detector.file_loaded(p("dir1/a"));
    detector.file_loaded(p("dir1/b"));
    detector.file_loaded(p("dir2/a"));
    detector.file_loaded(p("dir2/b"));
    assert_eq!(detector.file_walks(), vec![(p(""), 1)]);
    // Directory walk is redundant - remove it.
    assert_eq!(detector.dir_walks(), vec![]);

    detector.file_loaded(p("dir1/dir1_1/a"));
    detector.file_loaded(p("dir1/dir1_1/b"));
    detector.file_loaded(p("dir2/dir2_1/a"));
    detector.file_loaded(p("dir2/dir2_1/b"));
    assert_eq!(detector.file_walks(), vec![(p(""), 2)]);
    assert_eq!(detector.dir_walks(), vec![]);

    detector.dir_loaded(p("dir1/dir1_1/dir1_1_1"), 2, 2);
    detector.dir_loaded(p("dir1/dir1_1/dir1_1_2"), 2, 2);
    assert_eq!(detector.file_walks(), vec![(p(""), 2)]);
    // No dir walk - depth=2 is already covered by file walk.
    assert_eq!(detector.dir_walks(), vec![]);
}

#[test]
fn test_walk_changed() {
    let detector = Detector::new();
    detector.set_walk_threshold(TEST_WALK_THRESHOLD);
    detector.set_gc_interval(Duration::from_secs(1));
    detector.set_gc_timeout(Duration::from_secs(2));

    let mut epoch = Instant::now();
    detector.set_now(epoch);

    // No walk.
    assert!(!detector.file_loaded(p("dir1/a")));

    // Yes walk.
    assert!(detector.file_loaded(p("dir1/b")));

    // No walk changes.
    assert!(!detector.file_loaded(p("dir2/a")));

    epoch += Duration::from_secs(5);
    detector.set_now(epoch);

    // GC removes walk
    assert!(detector.file_loaded(p("dir2/a")));
}

#[test]
fn test_touched() {
    let detector = Detector::new();
    detector.set_walk_threshold(TEST_WALK_THRESHOLD);
    detector.set_gc_interval(Duration::from_secs(1));
    detector.set_gc_timeout(Duration::from_secs(2));

    let mut epoch = Instant::now();
    detector.set_now(epoch);

    detector.file_loaded(p("dir1/a"));
    detector.file_loaded(p("dir1/b"));
    assert_eq!(detector.file_walks().len(), 1);

    detector.dir_loaded(p("dir2/a"), 0, 0);
    detector.dir_loaded(p("dir2/b"), 0, 0);
    assert_eq!(detector.dir_walks().len(), 1);

    for _ in 0..10 {
        epoch += Duration::from_secs(1);
        detector.set_now(epoch);

        assert!(detector.file_read(p("dir1/c")));
        assert!(detector.dir_read(p("dir2/c")));
    }

    detector.file_loaded(p("something/else"));
    detector.dir_loaded(p("something/else"), 0, 0);

    // Walks still around - not GC'd.
    assert_eq!(detector.file_walks().len(), 1);
    assert_eq!(detector.dir_walks().len(), 1);

    // Test that the touched methods don't resurrect expired (but not collected) nodes.
    epoch += Duration::from_secs(5);
    detector.set_now(epoch);
    assert!(!detector.file_read(p("dir1/c")));
    assert!(detector.file_walks().is_empty());
    assert!(detector.dir_walks().is_empty());
}

#[test]
fn test_counters() {
    let detector = Detector::new();
    detector.set_walk_threshold(TEST_WALK_THRESHOLD);

    let get_counters = |p: RepoPathBuf, wt: WalkType| {
        detector
            .inner
            .write()
            .node
            .get_node(&p)
            .and_then(|n| n.get_walk_for_type(wt))
            .map(|w| w.counters())
            .unwrap()
    };

    // Include initial counts when we first create the walk.
    detector.dir_loaded(p("dir1/dir1_1"), 0, 0);
    detector.dir_loaded(p("dir1/dir1_2"), 0, 0);
    detector.dir_loaded(p("dir1/dir1_2"), 0, 0);
    detector.dir_read(p("dir1/dir1_2"));
    assert_eq!(
        get_counters(p("dir1"), WalkType::Directory),
        (0, 0, 0, 3, 1)
    );

    // Propagate counts when we convert to file walk.
    detector.file_loaded(p("dir1/a"));
    detector.file_loaded(p("dir1/b"));
    detector.files_preloaded(p("dir1"), 1);
    assert_eq!(get_counters(p("dir1"), WalkType::File), (2, 0, 1, 3, 1));

    // Propagate when combining walks.
    detector.file_loaded(p("dir2/a"));
    detector.file_loaded(p("dir2/b"));
    assert_eq!(get_counters(p(""), WalkType::File), (4, 0, 1, 3, 1));
}

#[test]
fn test_stricter_threshold() {
    let detector = Detector::new();
    detector.set_walk_threshold(TEST_WALK_THRESHOLD);
    detector.set_lax_depth(1);
    detector.set_strict_multiplier(2);

    detector.file_loaded(p("dir1/a"));
    detector.file_loaded(p("dir1/b"));

    detector.file_loaded(p("dir2/a"));
    detector.file_loaded(p("dir2/b"));

    // Walk didn't get propagated to root due to stricter threshold above depth 1.
    assert_eq!(detector.file_walks(), vec![(p("dir1"), 0), (p("dir2"), 0)]);

    detector.file_loaded(p("dir3/a"));
    detector.file_loaded(p("dir3/b"));

    detector.file_loaded(p("dir4/a"));
    detector.file_loaded(p("dir4/b"));

    // Still eventually gets propagated.
    assert_eq!(detector.file_walks(), vec![(p(""), 1)]);
}
