/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use super::TestDag;

#[tokio::test]
async fn test_strip_basic() {
    // Test strip C (and its descendants D, E), then H.
    // C, D are in the master group. E, F::I are in non-master.
    let mut dag = TestDag::draw_client(
        r#"
        A--B--C--D--E
         \
          F--G--H--I
        # master: D"#,
    )
    .await;

    // Note "B" is not known locally - it's lazy.
    let state_before_strip = dag.dump_state().await;
    assert_eq!(
        &state_before_strip,
        r#"<spans [F:E+N0:N4, A:D+0:3]>
Lv0: RH0-3[], N0-N3[0], N4-N4[3]
P->C: 0->N0, 3->N4
0->A 3->D N0->F N1->G N2->H N3->I N4->E"#
    );

    // Strip C.
    // Note "B" becomes the new "head" and is currently lazy.
    // So strip needs extra work to ensure "B" is non-lazy.
    dag.strip("C").await;

    let state_after_strip = dag.dump_state().await;
    assert_eq!(
        &state_after_strip,
        r#"<spans [F:I+N0:N3, A:B+0:1]>
Lv0: RH0-1[], N0-N3[0]
P->C: 0->N0
0->A 1->B N0->F N1->G N2->H N3->I"#
    );

    // Check that strip is persisted and can be seen after reopen.
    dag.reopen();
    assert_eq!(&dag.dump_state().await, &state_after_strip);

    // Strip H. The F-I segment is truncated to F-G.
    dag.strip("H").await;
    assert_eq!(
        dag.dump_state().await,
        r#"<spans [F:G+N0:N1, A:B+0:1]>
Lv0: RH0-1[], N0-N1[0]
P->C: 0->N0
0->A 1->B N0->F N1->G"#
    );
}

#[tokio::test]
async fn test_strip_branch_and_parent_remains_lazy() {
    // Test that stripping a (broken) branch does not resolve
    // the branching point. This allows strip to remove "broken"
    // branches (ex. removed by server) without resolving new
    // vertexes remotely, because the remote protocol might not
    // work.
    let ascii = r#"
        A--B--C--D--E
            \
             F--G
        # master: D F"#;

    let dag = TestDag::draw_client(ascii).await;
    assert_eq!(
        dag.dump_state().await,
        r#"<spans [G:E+N0:N1, 0:4]>
Lv0: RH0-3[], 4-4[1], N0-N0[4], N1-N1[3]
P->C: 1->4, 3->N1, 4->N0
3->D 4->F N0->G N1->E"#
    );

    let strip = |s: &'static str| async move {
        let mut dag = TestDag::draw_client(ascii).await;
        dag.strip(s).await;
        dag.dump_state().await
    };

    // Strip C. B is still lazy because it's not a head.
    assert_eq!(
        strip("C").await,
        r#"<spans [G+N0, F+4, 0:1]>
Lv0: RH0-1[], 4-4[1], N0-N0[4]
P->C: 1->4, 4->N0
4->F N0->G"#
    );

    // Strip F. B is still lazy as well.
    assert_eq!(
        strip("F").await,
        "<spans [E+N1, 0:3]>\nLv0: RH0-3[], N1-N1[3]\nP->C: 3->N1\n3->D N1->E"
    );

    // Strip C+F. B is no longer lazy.
    assert_eq!(strip("C F").await, "<spans [0:1]>\nLv0: RH0-1[]\n1->B");
}

#[tokio::test]
async fn test_reinsert_then_create_higher_level() {
    // Test re-inserting a stripped "gap" and then creating higher level segments.
    //
    // Initial state:
    //
    // Lv0: |0|1|2|3|4|5|6|7|8|9|10|
    // Lv1: |-|---|-|---|---|---|
    // Lv2: |-----|-|-------|
    //
    // Strip 3:
    //
    // Lv0: |0|1|2| |4|5|6|7|8|9|10|
    // Lv1: |-|---| |---|---|---|
    // Lv2: |-----| |-------|
    //
    // Reinsert 3 (Note: 3 no longer has Lv1 and Lv2 segments):
    //
    // Lv0: |0|1|2|3|4|5|6|7|8|9|10|
    // Lv1: |-|---| |---|---|---|
    // Lv2: |-----| |-------|
    //
    // Trigger Lv3 segment creation:
    //
    // Lv0: |0|1|2|3|4|5|6|7|8|9|10|11|12|13|14|
    // Lv1: |-|---| |---|---|---|-----|-----|
    // Lv2: |-----| |-------|---------|
    // Lv3: ?       |-----------------|

    let mut dag = TestDag::new_with_segment_size(2);

    // Use "Z" as an extra parent to avoid merging flat segments.
    dag.drawdag("Z", &[]);
    for i in 1..=10 {
        let ascii = match i {
            1 | 3 | 4 => format!("A{i}", i = i),
            _ => format!("Z-A{i} A{p}-A{i}", p = i - 1, i = i),
        };
        dag.drawdag(&ascii, &[]);
    }
    dag.flush("").await;

    assert_eq!(
        dag.dump_segments_ascii(),
        r#"
        Lv0: |N0|N1|N2|N3|N4|N5|N6|N7|N8|N9|N10|
        Lv1: |N0|N1 N2|N3|N4 N5|N6 N7|N8 N9|
        Lv2: |N0 N1 N2|N3|N4 N5 N6 N7|"#
    );

    // Strip 3.
    dag.strip("A3").await;
    assert_eq!(
        dag.dump_segments_ascii(),
        r#"
        Lv0: |N0|N1|N2| |N4|N5|N6|N7|N8|N9|N10|
        Lv1: |N0|N1 N2| |N4 N5|N6 N7|N8 N9|
        Lv2: |N0 N1 N2| |N4 N5 N6 N7|"#
    );

    // Reinsert 3. Note A3 does not have Lv1 or Lv2 segments.
    dag.drawdag("A2-A3 Z-A3", &[]);
    assert_eq!(
        dag.dump_segments_ascii(),
        r#"
        Lv0: |N0|N1|N2|N3|N4|N5|N6|N7|N8|N9|N10|
        Lv1: |N0|N1 N2|  |N4 N5|N6 N7|N8 N9|
        Lv2: |N0 N1 N2|  |N4 N5 N6 N7|"#
    );

    // Try to create Lv3 segments. This does not crash.
    dag.drawdag("A10-A11 A3-A11", &[]);
    for i in 12..=18 {
        let ascii = format!("Z-A{i} A{p}-A{i}", p = i - 1, i = i);
        dag.drawdag(&ascii, &[]);
    }
    assert_eq!(
        dag.dump_segments_ascii(),
        r#"
        Lv0: |N0|N1|N2|N3|N4|N5|N6|N7|N8|N9|N10|N11|N12|N13|N14|N15|N16|N17|N18|
        Lv1: |N0|N1 N2|  |N4 N5|N6 N7|N8 N9|N10 N11|N12 N13|N14 N15|N16 N17|
        Lv2: |N0 N1 N2|  |N4 N5 N6 N7|N8 N9 N10 N11|N12 N13 N14 N15|
        Lv3: |N0 N1 N2|  |N4 N5 N6 N7 N8 N9 N10 N11|"#
    );
}
