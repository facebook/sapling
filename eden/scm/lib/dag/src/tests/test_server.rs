/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Tests cases that are more relavant for server use-cases.
//!
//! The server use-cases are different from the client in a few ways:
//! - The server has a non-lazy `IdMap`.
//! - The server might store `IdMap` in SQL without using transactions (because
//!   of performance concerns and size limits). That could cause `IdMap` having
//!   more entries than `IdDag`. The client uses `MultiLog` in indexedlog to
//!   ensure `IdMap` and `IdDag` are in sync.

use crate::idmap::IdMapWrite;
use crate::ops::DagAddHeads;
use crate::tests::DrawDag;
use crate::tests::TestDag;
use crate::Group;
use crate::Id;
use crate::VertexListWithOptions;

#[tokio::test]
async fn test_idmap_more_entries_than_iddag() {
    let mut dag = TestDag::draw("A--B  # master: B");

    // Make IdMap contain more entries than IdDag.
    let map = &mut dag.dag.map;
    IdMapWrite::insert(map, Id(5), b"C").await.unwrap();
    IdMapWrite::insert(map, Id(6), b"D").await.unwrap();
    IdMapWrite::insert(map, Id(8), b"E").await.unwrap();
    IdMapWrite::insert(map, Id(20), b"H").await.unwrap();
    IdMapWrite::insert(map, Id(21), b"I").await.unwrap();

    let draw = DrawDag::from(
        r#"
           B--C--D--F--G  H--I--J--K
                  /
                 E"#,
    );
    let heads = VertexListWithOptions::from(&["G".into(), "K".into()][..])
        .with_highest_group(Group::MASTER);
    dag.dag.add_heads(&draw, &heads).await.unwrap();

    // C, D, E, H, I are inserted to the IdDag respecting their IdMap values.
    assert_eq!(
        dag.debug_segments(0, Group::MASTER),
        r#"
        H+20 : K+23 [] Root
        F+9 : G+10 [D+6, E+8]
        E+8 : E+8 [] Root
        C+5 : D+6 [B+1]
        A+0 : B+1 [] Root OnlyHead"#
    );
}

#[tokio::test]
async fn test_idmap_more_entries_conflict_with_assign_head() {
    let mut dag = TestDag::draw("A--B  # master: B");

    // Make IdMap contain entry that conflicts with new Id assignment.
    let map = &mut dag.dag.map;
    IdMapWrite::insert(map, Id(2), b"C").await.unwrap();

    let draw = DrawDag::from("B--D");
    let heads = VertexListWithOptions::from(&["D".into()][..]).with_highest_group(Group::MASTER);
    dag.dag.add_heads(&draw, &heads).await.unwrap();

    // D takes id 3, skips the taken id 2.
    assert_eq!(
        dag.debug_segments(0, Group::MASTER),
        "\n        D+3 : D+3 [B+1]\n        A+0 : B+1 [] Root OnlyHead"
    );
}
