/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Tests about discontinuous segments
//!
//! Previously, segments in a group are continuous. In other words, all segments
//! in the master group can be represented using a single span `0..=x`.  With
//! discontinuous segments, a group might be represented as a few spans.
//!
//! The discontinuous spans are designed to better support multiple long-lived
//! long branches. For example:
//!
//! ```plain,ignore
//! 1---2---3--...--999---1000     branch1
//!      \
//!       5000--...--5999---6000   branch2
//! ```
//!
//! Note: discontinuous segments is not designed to support massive amount of
//! branches. It introduces O(branch) factor in complexity in many places.

use crate::ops::DagAddHeads;
use crate::ops::DagAlgorithm;
use crate::ops::DagPersistent;
use crate::tests::DrawDag;
use crate::tests::TestDag;
use crate::Group;
use crate::Vertex;
use crate::VertexListWithOptions;
use crate::VertexOptions;

#[tokio::test]
async fn test_simple_3_branches() {
    let mut dag = TestDag::new();
    let draw = DrawDag::from(
        r#"
            A--B--C--D--E--F
                   \
                    G--H--I
                     \
                      J--K--L"#,
    );

    let heads = VertexListWithOptions::from(vec![
        reserved_head("F", 100),
        reserved_head("I", 50),
        reserved_head("L", 0),
    ]);

    dag.dag.add_heads_and_flush(&draw, &heads).await.unwrap();
    assert_eq!(
        format!("{:?}", &dag.dag),
        r#"Max Level: 0
 Level 0
  Group Master:
   Segments: 3
    J+159 : L+161 [G+106]
    G+106 : I+108 [C+2]
    A+0 : F+5 [] Root OnlyHead
  Group Non-Master:
   Segments: 0
"#
    );

    assert_eq!(
        format!("{:?}", dag.dag.ancestors("I".into()).await.unwrap()),
        "<spans [G:I+106:108, A:C+0:2]>"
    );
    assert_eq!(
        format!("{:?}", dag.dag.descendants("B".into()).await.unwrap()),
        "<spans [J:L+159:161, G:I+106:108, B:F+1:5]>"
    );
    assert_eq!(
        format!("{:?}", dag.dag.range("C".into(), "K".into()).await.unwrap()),
        "<spans [J:K+159:160, G+106, C+2]>"
    );
    assert_eq!(
        format!("{:?}", dag.dag.parents("G".into()).await.unwrap()),
        "<spans [C+2]>"
    );
    assert_eq!(
        format!("{:?}", dag.dag.children("C".into()).await.unwrap()),
        "<spans [G+106, D+3]>"
    );

    let all = dag.dag.all().await.unwrap();
    assert_eq!(
        format!("{:?}", dag.dag.children(all.clone()).await.unwrap()),
        "<spans [J:L+159:161, G:I+106:108, B:F+1:5]>"
    );
    assert_eq!(
        format!("{:?}", dag.dag.parents(all.clone()).await.unwrap()),
        "<spans [J:K+159:160, G:H+106:107, A:E+0:4]>"
    );
    assert_eq!(
        format!(
            "{:?}",
            dag.dag.range(all.clone(), all.clone()).await.unwrap()
        ),
        "<spans [J:L+159:161, G:I+106:108, A:F+0:5]>"
    );
}

#[tokio::test]
async fn test_grow_branches() {
    let mut dag = TestDag::new();
    let draw = DrawDag::from(
        r#"
            A0-A1-A2
            B0-B1-B2
            C0-C1-C2"#,
    );
    let heads = VertexListWithOptions::from(vec![
        reserved_head("A2", 4),
        reserved_head("B2", 4),
        reserved_head("C2", 4),
    ]);
    dag.dag.add_heads_and_flush(&draw, &heads).await.unwrap();
    assert_eq!(
        format!("{:?}", dag.dag.all().await.unwrap()),
        "<spans [C0:C2+14:16, B0:B2+7:9, A0:A2+0:2]>"
    );

    // Grow all branches with larger reservation. Larger reservation
    // is changed to pre-existing reservation to avoid fragmentation.
    let draw = DrawDag::from(
        r#" A2-A3-A4
            B2-B3-B4
            C2-C3-C4"#,
    );
    let heads = VertexListWithOptions::from(vec![
        reserved_head("A4", 20),
        reserved_head("B4", 20),
        reserved_head("C4", 20),
    ]);
    dag.dag.add_heads_and_flush(&draw, &heads).await.unwrap();
    assert_eq!(
        format!("{:?}", dag.dag.all().await.unwrap()),
        "<spans [C0:C4+14:18, B0:B4+7:11, A0:A4+0:4]>"
    );

    // Large reservation is respected when the previous reservation
    // gets used up. Note how B5 and C5 respects reservations from
    // A8 (->C4) and B6.
    let draw = DrawDag::from(
        r#" A4-A5-A6-A7-A8
            B4-B5-B6
            C4-C5-C6-C7-C8"#,
    );
    let heads = VertexListWithOptions::from(vec![
        reserved_head("A8", 20),
        reserved_head("B6", 20),
        reserved_head("C8", 20),
    ]);
    dag.dag.add_heads(&draw, &heads).await.unwrap();
    assert_eq!(
        dag.debug_segments(0, Group::MASTER),
        r#"
        C5+61 : C8+64 [C4+18]
        B5+39 : B6+40 [B4+11]
        C0+14 : C4+18 [] Root
        A7+12 : A8+13 [A6+6]
        B0+7 : B4+11 [] Root
        A0+0 : A6+6 [] Root OnlyHead"#
    );
}

#[tokio::test]
async fn test_reservation_on_existing_vertex() {
    let mut dag = TestDag::new();
    let draw = DrawDag::from(
        r#" A0-A1-A2
            B0-B1-B2"#,
    );
    let heads = VertexListWithOptions::from(vec![reserved_head("A2", 4)]);
    dag.dag.add_heads_and_flush(&draw, &heads).await.unwrap();
    assert_eq!(
        format!("{:?}", dag.dag.all().await.unwrap()),
        "<spans [A0:A2+0:2]>"
    );

    // A2: 4 is respected (3..=6 is reserved).
    let heads = VertexListWithOptions::from(vec![reserved_head("B2", 4), reserved_head("A2", 4)]);
    dag.dag.add_heads(&draw, &heads).await.unwrap();
    assert_eq!(
        format!("{:?}", dag.dag.all().await.unwrap()),
        "<spans [B0:B2+7:9, A0:A2+0:2]>"
    );
}

fn reserved_head(s: &'static str, reserve_size: u32) -> (Vertex, VertexOptions) {
    (
        Vertex::from(s),
        VertexOptions {
            reserve_size,
            highest_group: Group::MASTER,
            ..Default::default()
        },
    )
}
