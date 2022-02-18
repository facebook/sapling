/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
