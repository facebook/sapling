/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::time::Duration;

use mock_instant::MockClock;
use types::RepoPath;
use types::RepoPathBuf;

use crate::Config;
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
    let mut detector = Detector::new();
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
    let mut detector = Detector::new();
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
    let mut detector = Detector::new();
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
    let config = Config::default();
    let mut node = WalkNode::new(config.gc_timeout);

    node.insert_walk(&config, WalkType::File, &p("foo"), Walk::new(1), 0);
    // Can re-insert.
    node.insert_walk(&config, WalkType::File, &p("foo"), Walk::new(1), 0);
    assert_eq!(node.list_walks(WalkType::File), vec![(p("foo"), 1)]);

    // Don't insert since it is fully contained by "foo" walk.
    node.insert_walk(&config, WalkType::File, &p("foo/bar"), Walk::new(0), 0);
    assert_eq!(node.list_walks(WalkType::File), vec![(p("foo"), 1)]);

    let baz_walk = Walk::new(2);
    node.insert_walk(&config, WalkType::File, &p("foo/bar/baz"), baz_walk, 0);
    assert_eq!(
        node.list_walks(WalkType::File),
        vec![(p("foo"), 1), (p("foo/bar/baz"), 2)]
    );

    let root_walk = Walk::new(0);
    node.insert_walk(&config, WalkType::File, &p(""), root_walk, 0);
    assert_eq!(
        node.list_walks(WalkType::File),
        vec![(p(""), 0), (p("foo"), 1), (p("foo/bar/baz"), 2)]
    );

    // depth=1 doesn't contain any descendant walks - don't clear anything out.
    let root_walk = Walk::new(1);
    node.insert_walk(&config, WalkType::File, &p(""), root_walk, 0);
    assert_eq!(
        node.list_walks(WalkType::File),
        vec![(p(""), 1), (p("foo"), 1), (p("foo/bar/baz"), 2)]
    );

    // depth=2 contains the "foo" walk - clear "foo" out.
    let root_walk = Walk::new(2);
    node.insert_walk(&config, WalkType::File, &p(""), root_walk, 0);
    assert_eq!(
        node.list_walks(WalkType::File),
        vec![(p(""), 2), (p("foo/bar/baz"), 2)]
    );

    // Contains the "foo/bar/baz" walk.
    let root_walk = Walk::new(5);
    node.insert_walk(&config, WalkType::File, &p(""), root_walk, 0);
    assert_eq!(node.list_walks(WalkType::File), vec![(p(""), 5)]);
}

#[test]
fn test_walk_node_get() {
    let config = Config::default();
    let mut node = WalkNode::new(config.gc_timeout);

    let get_depth = |node: &mut WalkNode, path: &RepoPath| -> Option<usize> {
        node.get_node(path)
            .and_then(|node| node.file_walk.as_ref().map(|walk| walk.depth))
    };

    assert!(get_depth(&mut node, &p("")).is_none());
    assert!(get_depth(&mut node, &p("foo")).is_none());
    assert!(get_depth(&mut node, &p("foo/bar")).is_none());

    let foo_walk = Walk::new(1);
    node.insert_walk(&config, WalkType::File, &p("foo"), foo_walk, 0);

    assert!(get_depth(&mut node, &p("")).is_none());
    assert_eq!(get_depth(&mut node, &p("foo")), Some(1));
    assert!(get_depth(&mut node, &p("foo/bar")).is_none());

    let foo_bar_walk = Walk::new(2);
    node.insert_walk(&config, WalkType::File, &p("foo/bar"), foo_bar_walk, 0);

    assert!(get_depth(&mut node, &p("")).is_none());
    assert_eq!(get_depth(&mut node, &p("foo")), Some(1));
    assert_eq!(get_depth(&mut node, &p("foo/bar")), Some(2));

    let root_walk = Walk::new(0);
    node.insert_walk(&config, WalkType::File, &p(""), root_walk, 0);

    assert_eq!(get_depth(&mut node, &p("")), Some(0));
    assert_eq!(get_depth(&mut node, &p("foo")), Some(1));
    assert_eq!(get_depth(&mut node, &p("foo/bar")), Some(2));
}

#[test]
fn test_walk_get_containing_node() {
    let config = Config::default();
    let mut node = WalkNode::new(config.gc_timeout);

    let dir = p("foo/bar/baz");

    assert!(node.get_owning_node(WalkType::File, &dir).is_none());

    node.insert_walk(&config, WalkType::File, &p("foo/bar"), Walk::new(0), 0);

    // Still not containing due to depth.
    assert!(node.get_owning_node(WalkType::File, &dir).is_none());

    node.insert_walk(&config, WalkType::File, &p("foo/bar"), Walk::new(1), 0);

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
    let mut detector = Detector::new();
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
    let mut detector = Detector::new();
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
    let mut detector = Detector::new();
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
fn test_retain_interesting_metadata_when_covered_by_walk() {
    let mut detector = Detector::new();
    detector.set_walk_threshold(TEST_WALK_THRESHOLD);
    detector.set_walk_ratio(0.1);

    // Start off with a directory walk at root/ triggered by un-interesting directories.
    detector.dir_loaded(p("root/dir1"), 0, 0);
    detector.dir_loaded(p("root/dir2"), 0, 0);
    assert_eq!(detector.dir_walks(), vec![(p("root"), 0)]);

    // Now we see a massive root/dir3, but it is already covered by root/ walk.
    // Be sure to remember metadata in this case.
    detector.dir_loaded(p("root/dir3"), 100, 0);

    // Don't create a file walk since we remembered that root/dir3 is very large.
    detector.file_loaded(p("root/dir3/a"));
    detector.file_loaded(p("root/dir3/b"));
    assert_eq!(detector.file_walks(), vec![]);
}

#[test]
fn test_dont_retain_empty_directory_metadata() {
    let mut detector = Detector::new();
    detector.set_walk_threshold(TEST_WALK_THRESHOLD);

    detector.dir_loaded(p("root/dir1"), 0, 0);

    // Check we didn't create a node for root/dir1. Empty is not interesting.
    assert!(
        detector
            .inner
            .read()
            .node
            .get_node(&p("root/dir1"))
            .is_none()
    );
}

#[test]
fn test_merge_cousins() {
    let mut detector = Detector::new();
    detector.set_walk_threshold(TEST_WALK_THRESHOLD);

    detector.dir_loaded(p("root"), 0, 1);

    // Walk at root/, depth=2.
    detector.file_loaded(p("root/foo/dir1/a"));
    detector.file_loaded(p("root/foo/dir1/b"));
    detector.file_loaded(p("root/foo/dir2/a"));
    detector.file_loaded(p("root/foo/dir2/b"));
    assert_eq!(detector.file_walks(), vec![(p("root"), 2)]);

    // These two cousin directories get merged to advance the root/ walk.
    detector.file_loaded(p("root/foo/dir1/dir1_1/a"));
    detector.file_loaded(p("root/foo/dir1/dir1_1/b"));
    detector.file_loaded(p("root/foo/dir2/dir2_1/a"));
    detector.file_loaded(p("root/foo/dir2/dir2_1/b"));
    assert_eq!(detector.file_walks(), vec![(p("root"), 3)]);
}

#[test]
fn test_dont_merge_into_containg_walk() {
    let mut detector = Detector::new();
    detector.set_walk_threshold(1);

    detector.file_loaded(p("a"));
    detector.file_loaded(p("dir1/a"));
    detector.file_loaded(p("dir1/dir1_1/a"));
    assert_eq!(detector.file_walks(), vec![(p(""), 2)]);

    detector.set_walk_threshold(TEST_WALK_THRESHOLD);

    detector.dir_loaded(p("dir2"), 0, 1000);
    detector.dir_loaded(p("dir2/dir2_1"), 0, 1);
    detector.dir_loaded(p("dir2/dir2_1/dir2_1_1"), 1, 0);
    detector.file_loaded(p("dir2/dir2_1/dir2_1_1/a"));
    assert_eq!(
        detector.file_walks(),
        vec![(p(""), 2), (p("dir2/dir2_1"), 1)]
    );

    detector.dir_loaded(p("dir2/dir2_2"), 0, 1);
    detector.dir_loaded(p("dir2/dir2_2/dir2_2_1"), 1, 0);
    detector.file_loaded(p("dir2/dir2_2/dir2_2_1/a"));

    // Be careful to not let the latter two walks merge into the root walk. "dir2" is a large
    // directory, so we shouldn't let "dir2/dir2_1" and "dir2/dir2_2" jump across it and deepen the
    // root walk.
    assert_eq!(
        detector.file_walks(),
        vec![(p(""), 2), (p("dir2/dir2_1"), 1), (p("dir2/dir2_2"), 1)]
    );
}

#[test]
fn test_gc() {
    // Test with a low interval so full GC is invoked often, and test with a large interval so full
    // GC is not invoked.
    for interval in [1, 100] {
        let mut detector = Detector::new();
        detector.set_walk_threshold(TEST_WALK_THRESHOLD);
        detector.set_gc_interval(Duration::from_secs(interval));
        detector.set_gc_timeout(Duration::from_secs(2));

        detector.file_loaded(p("dir1/a"));
        assert_eq!(detector.file_walks(), vec![]);

        MockClock::advance(Duration::from_secs(1));

        // GC should run but not remove anything.
        detector.file_loaded(p("dir1/b"));
        assert_eq!(detector.file_walks(), vec![(p("dir1"), 0)]);

        MockClock::advance(Duration::from_secs(1));

        // This should keep dir1 walk alive.
        detector.file_loaded(p("dir1/c"));
        detector.file_loaded(p("dir2/a"));
        detector.file_loaded(p("some/deep/dir/a"));
        assert_eq!(detector.file_walks(), vec![(p("dir1"), 0)]);

        MockClock::advance(Duration::from_secs(1));

        detector.file_loaded(p("dir2/b"));
        assert_eq!(detector.file_walks(), vec![(p(""), 1)]);

        MockClock::advance(Duration::from_secs(1));

        // GC should clear out some/deep/dir, so this should not result in walk.
        detector.file_loaded(p("some/deep/dir/b"));
        // This should update access time for root walk.
        detector.file_loaded(p("dir3/a"));
        assert_eq!(detector.file_walks(), vec![(p(""), 1)]);

        // Root walk still here since dir3/a refreshed access time.
        MockClock::advance(Duration::from_secs(1));
        assert_eq!(detector.file_walks(), vec![(p(""), 1)]);

        // Everything is GC'd.
        MockClock::advance(Duration::from_secs(1));
        assert_eq!(detector.file_walks(), vec![]);
    }
}

#[test]
fn test_gc_stats() {
    let mut detector = Detector::new();
    detector.set_walk_threshold(TEST_WALK_THRESHOLD);

    // Don't run GC automatically.
    detector.set_gc_interval(Duration::from_secs(10));

    detector.set_gc_timeout(Duration::from_secs(1));

    detector.file_loaded(p("dir1/a"));
    detector.file_loaded(p("dir1/b"));

    detector.file_loaded(p("dir2/a"));

    detector.file_loaded(p("dir3/dir4/a"));

    MockClock::advance(Duration::from_secs(2));

    // Refresh access time on dir3.
    detector.file_loaded(p("dir3/dir4/b"));

    // Manually run GC to check stats.
    let (nodes_removed, nodes_remaining, walks_removed) =
        detector.inner.write().node.gc(&Default::default());

    // "dir1" and "dir2"
    assert_eq!(nodes_removed, 2);

    // root node and "dir3" and "dir4"
    assert_eq!(nodes_remaining, 3);

    // "dir1"
    assert_eq!(walks_removed, 1);
}

#[test]
fn test_dir_walk() {
    let mut detector = Detector::new();
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
    let mut detector = Detector::new();
    detector.set_walk_threshold(TEST_WALK_THRESHOLD);
    detector.set_gc_interval(Duration::from_secs(1));
    detector.set_gc_timeout(Duration::from_secs(2));

    // No walk.
    assert!(!detector.file_loaded(p("dir1/a")));

    // Yes walk.
    assert!(detector.file_loaded(p("dir1/b")));

    // No walk changes.
    assert!(!detector.file_loaded(p("dir2/a")));

    MockClock::advance(Duration::from_secs(5));

    // GC removes walk
    assert!(detector.file_loaded(p("dir2/a")));
}

#[test]
fn test_touched() {
    let mut detector = Detector::new();
    detector.set_walk_threshold(TEST_WALK_THRESHOLD);
    detector.set_gc_interval(Duration::from_secs(1));
    detector.set_gc_timeout(Duration::from_secs(2));

    detector.file_loaded(p("dir1/a"));
    detector.file_loaded(p("dir1/b"));
    assert_eq!(detector.file_walks().len(), 1);

    detector.dir_loaded(p("dir2/a"), 0, 0);
    detector.dir_loaded(p("dir2/b"), 0, 0);
    assert_eq!(detector.dir_walks().len(), 1);

    for _ in 0..10 {
        MockClock::advance(Duration::from_secs(1));

        assert!(detector.file_read(p("dir1/c")));
        assert!(detector.dir_read(p("dir2/c")));
    }

    detector.file_loaded(p("something/else"));
    detector.dir_loaded(p("something/else"), 0, 0);

    // Walks still around - not GC'd.
    assert_eq!(detector.file_walks().len(), 1);
    assert_eq!(detector.dir_walks().len(), 1);

    // Test that the touched methods don't resurrect expired (but not collected) nodes.
    MockClock::advance(Duration::from_secs(5));
    assert!(!detector.file_read(p("dir1/c")));
    assert!(detector.file_walks().is_empty());
    assert!(detector.dir_walks().is_empty());
}

#[test]
fn test_counters() {
    let mut detector = Detector::new();
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
    let mut detector = Detector::new();
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

#[test]
fn test_huge_directory() {
    let mut detector = Detector::new();
    detector.set_walk_threshold(TEST_WALK_THRESHOLD);
    detector.set_walk_ratio(0.1);

    // Huge directory.
    detector.dir_loaded(p("dir1"), 100, 100);

    detector.file_loaded(p("dir1/a"));
    detector.file_loaded(p("dir1/b"));

    // No walks yet - 2/100 has not reached out threshold of 10%.
    assert!(detector.file_walks().is_empty());

    for i in 0..5 {
        detector.file_loaded(p(format!("dir1/{i}")));
    }

    // Still not enough.
    assert!(detector.file_walks().is_empty());

    for i in 0..10 {
        detector.file_loaded(p(format!("dir1/{i}")));
    }

    // Now we hit the 10% threshold.
    assert_eq!(detector.file_walks(), vec![(p("dir1"), 0)]);

    // Root directory needs 30*0.1=3 walked children to become walked.
    detector.dir_loaded(p(""), 0, 30);

    detector.file_loaded(p("dir2/a"));
    detector.file_loaded(p("dir2/b"));

    // Didn't bubble up yet even though we met the base walk threshold of 2.
    assert_eq!(detector.file_walks(), vec![(p("dir1"), 0), (p("dir2"), 0)]);

    // Now we meet the 10% limit.
    detector.file_loaded(p("dir3/a"));
    detector.file_loaded(p("dir3/b"));
    assert_eq!(detector.file_walks(), vec![(p(""), 1)]);
}

#[test]
fn test_slow_walk() {
    let mut detector = Detector::new();
    detector.set_walk_threshold(3);
    detector.set_gc_interval(Duration::from_secs(1));
    detector.set_gc_timeout(Duration::from_secs(3));

    detector.file_loaded(p("dir/a"));
    detector.file_loaded(p("dir/b"));
    detector.file_loaded(p("dir/c"));
    assert_eq!(detector.file_walks(), vec![(p("dir"), 0)]);

    // Slowly (relative to GC timeout) advance the walk. Make sure the "dir" walk doesn't time out
    // while we are making progress.
    MockClock::advance(Duration::from_secs(2));

    detector.file_loaded(p("dir/dir_1/a"));
    detector.file_loaded(p("dir/dir_1/b"));
    detector.file_loaded(p("dir/dir_1/c"));
    assert_eq!(
        detector.file_walks(),
        vec![(p("dir"), 0), (p("dir/dir_1"), 0)]
    );

    MockClock::advance(Duration::from_secs(2));

    detector.file_loaded(p("dir/dir_2/a"));
    detector.file_loaded(p("dir/dir_2/b"));
    detector.file_loaded(p("dir/dir_2/c"));
    assert_eq!(
        detector.file_walks(),
        vec![(p("dir"), 0), (p("dir/dir_1"), 0), (p("dir/dir_2"), 0)]
    );

    MockClock::advance(Duration::from_secs(2));

    detector.file_loaded(p("dir/dir_3/a"));
    detector.file_loaded(p("dir/dir_3/b"));
    detector.file_loaded(p("dir/dir_3/c"));
    assert_eq!(detector.file_walks(), vec![(p("dir"), 1)]);
}

#[test]
fn test_split_off_child_walk() {
    let mut detector = Detector::new();
    detector.set_walk_threshold(TEST_WALK_THRESHOLD);
    detector.set_walk_ratio(0.1);

    detector.dir_loaded(p("big"), 5, 30);

    detector.file_loaded(p("small/a"));
    detector.file_loaded(p("small/b"));
    detector.file_loaded(p("big/a"));
    detector.file_loaded(p("big/b"));
    assert_eq!(detector.file_walks(), vec![(p(""), 1)]);

    detector.file_loaded(p("small/dir1/a"));
    detector.file_loaded(p("small/dir1/b"));
    detector.file_loaded(p("big/dir1/a"));
    detector.file_loaded(p("big/dir1/b"));
    assert_eq!(detector.file_walks(), vec![(p(""), 2)]);

    detector.file_loaded(p("small/dir1/dir1_1/a"));
    detector.file_loaded(p("small/dir1/dir1_1/b"));
    detector.file_loaded(p("big/dir1/dir1_1/a"));
    detector.file_loaded(p("big/dir1/dir1_1/b"));
    assert_eq!(detector.file_walks(), vec![(p(""), 3)]);

    // Now "small" has run out of depth, but "big" still has a lot left.

    detector.file_loaded(p("big/dir2/dir2_1/dir2_1_1/a"));
    detector.file_loaded(p("big/dir2/dir2_1/dir2_1_1/b"));

    detector.file_loaded(p("big/dir3/dir3_1/dir3_1_1/a"));
    detector.file_loaded(p("big/dir3/dir3_1/dir3_1_1/b"));

    // Haven't hit the threshold of 3 yet (0.1*30=3)
    assert_eq!(
        detector.file_walks(),
        vec![
            (p(""), 3),
            (p("big/dir2/dir2_1/dir2_1_1"), 0),
            (p("big/dir3/dir3_1/dir3_1_1"), 0)
        ]
    );

    detector.file_loaded(p("big/dir4/dir4_1/dir4_1_1/a"));
    detector.file_loaded(p("big/dir4/dir4_1/dir4_1_1/b"));

    // No matter how much we see at depth=3, we won't advance the root walk since all advancements are under a single child "big".
    // But if we break off a separate walk for "big", then it can deepen on its own.
    assert_eq!(detector.file_walks(), vec![(p(""), 3), (p("big"), 3)]);
}

#[test]
fn test_important_metadata() {
    let mut detector = Detector::new();
    detector.set_walk_threshold(TEST_WALK_THRESHOLD);
    detector.set_walk_ratio(0.1);

    detector.dir_loaded(p("big"), 1000, 1000);

    MockClock::advance(Duration::from_secs(60));

    assert!(detector.file_walks().is_empty());
    detector.dir_read(p("big"));

    // Make sure we remembered the important metadata.
    assert_eq!(
        detector
            .inner
            .read()
            .node
            .get_node(&p("big"))
            .unwrap()
            .total_dirs
            .unwrap(),
        1000
    );
}
