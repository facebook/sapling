/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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

    // FIXME: Ids inserted manually are not present in IdDag.
    assert_eq!(
        dag.debug_segments(0, Group::MASTER),
        r#"
        J+22 : K+23 [I+21]
        F+9 : G+10 [D+6, E+8]
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
    let res = dag.dag.add_heads(&draw, &heads).await;

    // FIXME: Conflict is not handled.
    assert_eq!(
        res.unwrap_err().to_string(),
        "bug: new entry 2 = [68] conflicts with an existing entry 2 = [67]"
    );
}
