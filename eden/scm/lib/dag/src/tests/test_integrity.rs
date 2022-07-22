/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use super::TestDag;
use crate::ops::CheckIntegrity;
use crate::ops::DagAlgorithm;
use crate::Group;

#[tokio::test]
async fn test_isomorphic_graph_with_different_segments() {
    let mut dag1 = TestDag::new();
    let mut dag2 = TestDag::new();

    dag1.drawdag(
        r#"
        A--B--C--D--E    K--L--M
         \     \     \
          F--G--H--I--J"#,
        &["J", "M"],
    );
    dag2.drawdag(
        r#"
        A--B--C--D--E    K--L--M
         \     \     \
          F--G--H--I--J"#,
        &["K", "F", "B", "H", "D", "I", "E", "J", "M"],
    );

    assert_eq!(
        dag1.debug_segments(0, Group::MASTER),
        r#"
        K+10 : M+12 [] Root
        J+9 : J+9 [E+4, I+8] OnlyHead
        H+7 : I+8 [C+2, G+6]
        F+5 : G+6 [A+0]
        A+0 : E+4 [] Root OnlyHead"#
    );
    assert_eq!(
        dag2.debug_segments(0, Group::MASTER),
        r#"
        L+11 : M+12 [K+0]
        J+10 : J+10 [E+9, I+8]
        E+9 : E+9 [D+7]
        I+8 : I+8 [H+6]
        D+7 : D+7 [C+5]
        H+6 : H+6 [C+5, G+4]
        C+5 : C+5 [B+3]
        G+4 : G+4 [F+2]
        B+3 : B+3 [A+1]
        A+1 : F+2 [] Root
        K+0 : K+0 [] Root OnlyHead"#
    );

    let heads = dag1.dag.heads(dag1.dag.all().await.unwrap()).await.unwrap();
    assert_eq!(
        dag1.dag
            .check_isomorphic_graph(&dag2.dag, heads.clone())
            .await
            .unwrap(),
        [] as [&str; 0]
    );
}

#[tokio::test]
async fn test_non_isomorphic_graphs() {
    assert_eq!(
        quick_check_graphs("A--B--C--D", "A--D").await,
        ["range A::D with parents []: length mismatch: 4 != 2"]
    );
    assert_eq!(
        quick_check_graphs("A--B--C", "Z--C A--B--C").await,
        ["range A::C with parents []: merge mismatch: 0 != 1"]
    );
    assert_eq!(
        quick_check_graphs("A--B--C", "Z--B A--B--C").await,
        ["range A::C with parents []: merge mismatch: 0 != 1"]
    );
    assert_eq!(
        quick_check_graphs("A--B--C", "Z--A--B--C").await,
        ["range A::C with parents []: parents mismatch: [] != [Z]"]
    );
    assert_eq!(
        quick_check_graphs("A B C", "A B").await,
        ["range C::C with parents []: cannot resolve range on the other graph: VertexNotFound(C)"]
    );
}

async fn quick_check_graphs(ascii1: &str, ascii2: &str) -> Vec<String> {
    let dag1 = TestDag::draw(ascii1);
    let dag2 = TestDag::draw(ascii2);
    let heads = dag1.dag.heads(dag1.dag.all().await.unwrap()).await.unwrap();
    dag1.dag
        .check_isomorphic_graph(&dag2.dag, heads)
        .await
        .unwrap()
}
