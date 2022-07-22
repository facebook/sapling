/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::Arc;

use futures::TryStreamExt;

use super::ProtocolMonitor;
use super::TestDag;
use crate::ops::DagAddHeads;
use crate::ops::DagAlgorithm;
use crate::ops::DagExportPullData;
use crate::ops::DagImportPullData;
use crate::ops::DagPersistent;
use crate::ops::IdConvert;
use crate::Group;
use crate::Id;
use crate::VertexListWithOptions;
use crate::VertexName;

#[tokio::test]
async fn test_sparse_dag() {
    // In this test, we have 3 dags:
    // - server1: A complete dag. Emulates server-side.
    // - server2: An isomorphism of server1. But uses different ids.
    // - client: A sparse dag. Emulates client-side. Cloned from server2.
    //   Speaks remote protocol with server1 or server2.
    let mut server1 = TestDag::new();
    server1.drawdag(
        r#"
        A----B----C----D----E----M----X
         \       /         /
          F-----G--H--I---J
                         /
                     K--L"#,
        &["M"],
    );

    // server2 is an isomorphism of server1 with different ids.
    let mut server2 = TestDag::new();
    server2.drawdag("A-F-G-H-I", &["I"]);
    server2.drawdag("K-L-J I-J", &["J"]);
    server2.drawdag("A-B-C G-C", &["C"]);
    server2.drawdag("C-D-E-M-X J-E", &["M"]);

    for opt_remote_dag in vec![Some(server1.dag), None] {
        let mut client = server2.client_cloned_data().await;

        // Note: some ids (ex. 11) does not have matching name in its IdMap.
        // The server-side non-master (X) is not cloned.
        assert_eq!(
            format!("{:?}", &client.dag),
            r#"Max Level: 0
 Level 0
  Group Master:
   Segments: 6
    11 : M+12 [D+10, J+7] OnlyHead
    9 : D+10 [B+8, G+2]
    B+8 : B+8 [0]
    J+7 : J+7 [I+4, L+6] OnlyHead
    5 : L+6 [] Root
    0 : I+4 [] Root OnlyHead
  Group Non-Master:
   Segments: 0
"#
        );

        // The remote protocol could be backed by either server1 or server2.
        if let Some(remote_dag) = opt_remote_dag {
            let protocol = ProtocolMonitor {
                inner: Box::new(remote_dag),
                output: client.output.clone(),
            };
            client.dag.set_remote_protocol(Arc::new(protocol));
        }

        // With remote protocol. Be able to resolve id <-> names.
        assert_eq!(client.dag.vertex_name(Id(9)).await.unwrap(), "C".into());
        assert_eq!(client.dag.vertex_id("E".into()).await.unwrap(), Id(11));

        // NameSet iteration works too, and resolve Ids in batch.
        let all: Vec<VertexName> = {
            let all = client.dag.all().await.unwrap();
            let iter = all.iter().await.unwrap();
            iter.try_collect().await.unwrap()
        };
        assert_eq!(
            format!("{:?}", all),
            "[M, E, D, C, B, J, L, K, I, H, G, F, A]"
        );

        assert_eq!(
            client.output(),
            [
                "resolve paths: [D~1]",
                "resolve names: [E], heads: [M]",
                "resolve paths: [L~1, I~1, G~1(+2)]"
            ]
        );
    }
}

#[tokio::test]
async fn test_lazy_hash_on_non_master_group() {
    // In this test, we have 3 dags:
    // - server1: A dag.
    // - server2: A dag similar to server1 but in non-master group.
    // - client: A sparse dag. Emulates client-side. Cloned from server1.
    //   Speaks remote protocol with server2.
    let ascii_graph = r#"
        A--B--C--D--I
                  /
        E--F--G--H
        "#;
    let mut server1 = TestDag::new();
    server1.drawdag(ascii_graph, &["I"]);

    // server2 has a same graph but in the non-master group.
    let server2 = TestDag::draw(ascii_graph);

    // server2 can answer name->location queries.
    let client = server1.client_cloned_data().await.with_remote(&server2);
    assert_eq!(client.dag.vertex_id("C".into()).await.unwrap(), Id(2));
    assert_eq!(client.dag.vertex_id("F".into()).await.unwrap(), Id(5));
}

#[tokio::test]
async fn test_negative_cache() {
    let server = TestDag::draw("A-B  # master: B");

    let mut client = server.client_cloned_data().await;

    // Lookup "C" - not found.
    assert!(client.dag.vertex_id("C".into()).await.is_err());
    assert_eq!(client.output(), ["resolve names: [C], heads: [B]"]);

    // Lookup again - no need to resolve again.
    assert!(client.dag.vertex_id("C".into()).await.is_err());
    assert_eq!(client.output(), Vec::<String>::new());

    // The negative cache does not affect inserting the name.
    client.drawdag("B-C-D", &[]);
    assert!(client.dag.vertex_id("C".into()).await.is_ok());
}

#[tokio::test]
async fn test_add_heads() {
    let server = TestDag::draw("A-B  # master: B");
    let mut client = server.client_cloned_data().await;

    let pending = TestDag::draw("A-C B-C-D-E-F-G E-H-K I-J-K");
    let parents = pending.dag.dag_snapshot().unwrap();
    client
        .dag
        .add_heads(
            &parents,
            &vec![VertexName::from("G"), VertexName::from("K")].into(),
        )
        .await
        .unwrap();
    assert_eq!(
        client.output(),
        [
            "resolve names: [I, A], heads: [B]",
            "resolve names: [C], heads: [B]"
        ]
    );

    client.flush("G").await;
    assert_eq!(client.output(), ["resolve names: [I], heads: [B]"]);

    let mut client = server.client_cloned_data().await;
    let heads = VertexListWithOptions::from(&["K".into()][..])
        .with_highest_group(Group::MASTER)
        .chain(&["G".into()][..]);
    client
        .dag
        .add_heads_and_flush(&parents, &heads)
        .await
        .unwrap();
    assert_eq!(
        client.output(),
        [
            "resolve names: [I, A], heads: [B]",
            "resolve names: [C], heads: [B]"
        ]
    );
}

#[tokio::test]
async fn test_basic_pull() {
    let server = TestDag::draw("A-B-C-D  # master: D");
    let mut client = server.client().await;
    client.drawdag("A-B", &["B"]);

    let missing = server.dag.only("D".into(), "B".into()).await.unwrap();
    let pull_data = server.dag.export_pull_data(&missing).await.unwrap();

    let heads = VertexListWithOptions::from(&["D".into()][..]).with_highest_group(Group::MASTER);
    client
        .dag
        .import_pull_data(pull_data, &heads)
        .await
        .unwrap();

    assert_eq!(server.render_graph(), client.render_graph());
}

#[tokio::test]
async fn test_pull_remap() {
    // In this test client and server going to have different IDs, but isomorphic graphs
    let mut server = TestDag::new();
    server.drawdag(
        r#"
        A-B--C--D
           \   /
            F-G
    "#,
        &["D"],
    );
    server.drawdag("B-E", &["E"]);
    let mut client = server.client().await;
    client.drawdag("A-B-E", &["E"]);

    client.pull_ff_master(&server, "E", "D").await.unwrap();

    assert_eq!(
        client.output(),
        [
            "resolve names: [A], heads: []",
            "resolve names: [C, F], heads: [E]"
        ]
    );

    assert_eq!(
        client.render_graph(),
        "
            D    6
            ├─╮
            │ G  5
            │ │
            │ F  4
            │ │
            C │  3
            ├─╯
            │ E  2
            ├─╯
            B  1
            │
            A  0"
    );

    assert_eq!(
        server.render_graph(),
        "
            E  6
            │
            │ D    5
            │ ├─╮
            │ │ G  4
            │ │ │
            │ │ F  3
            ├───╯
            │ C  2
            ├─╯
            B  1
            │
            A  0"
    );
}

#[tokio::test]
async fn test_pull_overlap() {
    let mut server = TestDag::new();
    server.drawdag("A-B-C-D-E-F", &["F"]);
    let mut client = server.client().await;
    client.drawdag("A", &["A"]);

    client.pull_ff_master(&server, "A", "D").await.unwrap();

    let e = client.pull_ff_master(&server, "B", "F").await.unwrap_err();
    assert_eq!(e.to_string(), "NeedSlowPath: C exists in local graph");

    assert_eq!(
        client.render_graph(),
        r#"
            D  3
            │
            C  2
            │
            B  1
            │
            A  0"#
    );
}

#[tokio::test]
async fn test_pull_lazy_with_merges() {
    // Test fast-forward pull on a lazy graph with merges.
    let mut server = TestDag::new();

    // Initial state. Both client and server has just one vertex.
    server.drawdag("A", &["A"]);
    let mut client = server.client_cloned_data().await;

    // Take some IDs so IDs are different from client graph.
    server.drawdag("X-Y", &["Y"]);

    // Linear fast-forward. The client has a lazy graph.
    server.drawdag("A-B-C-D-E", &["E"]);
    client.pull_ff_master(&server, "A", "E").await.unwrap();
    assert_eq!(client.output(), ["resolve names: [B], heads: [A]"]);

    // C, D are lazy, but E is not.
    assert!(!client.contains_vertex_locally("C"));
    assert!(!client.contains_vertex_locally("D"));
    assert!(client.contains_vertex_locally("E"));

    // Add merges. Test parents remap.
    server.drawdag(
        r#"
                  D
                   \
        C E-F---G-H-I-J-K
         \     /   /
          L-M-N   M
        "#,
        &["K"],
    );
    assert_eq!(
        server.debug_segments(0, Group::MASTER),
        r#"
        I+13 : K+15 [D+5, H+12, M+8]
        G+11 : H+12 [F+9, N+10]
        N+10 : N+10 [M+8]
        F+9 : F+9 [E+6]
        L+7 : M+8 [C+4]
        B+3 : E+6 [A+0]
        X+1 : Y+2 [] Root
        A+0 : A+0 [] Root OnlyHead"#
    );

    client.pull_ff_master(&server, "E", "K").await.unwrap();
    assert_eq!(
        client.debug_segments(0, Group::MASTER),
        r#"
        I+11 : K+13 [D+3, H+10, M+7] OnlyHead
        G+9 : H+10 [F+5, N+8] OnlyHead
        L+6 : N+8 [C+2]
        A+0 : F+5 [] Root OnlyHead"#
    );
    assert_eq!(client.output(), ["resolve names: [C, D, F, L], heads: [E]"]);
}

#[tokio::test]
async fn test_pull_no_pending_changes() {
    let mut server = TestDag::draw("A # master: A");
    let mut client = server.client_cloned_data().await;
    server.drawdag("A-B-C", &["C"]);
    client.drawdag("A-D", &[]);
    let e = client.pull_ff_master(&server, "A", "C").await.unwrap_err();
    assert_eq!(
        e.to_string(),
        "ProgrammingError: import_pull_data called with pending heads ([D])"
    );
}

#[tokio::test]
async fn test_flush_reassign_master() {
    // Test remote calls when flush() causes id reassignment
    // from non-master to master.

    let mut server = TestDag::draw("A-B-C-D-E # master: E");
    let mut client = server.client_cloned_data().await;

    // Add vertexes in the non-master group.
    // There are 2 parts: ::G will be reassigned to the master group.
    // The rest (H, ::L) will remain in the non-master group.
    client.drawdag("B-X-Y-Z-F D-F-G-H B-I-J-K-L", &[]);
    assert_eq!(
        client.output(),
        [
            "resolve names: [B, D], heads: [E]",
            "resolve names: [I, X], heads: [E]",
            "resolve paths: [E~2, E~4]"
        ]
    );

    client.flush("").await;
    assert!(client.output().is_empty());

    // The server needs to have the new master group vertexes (up to G)
    // for the client to be able to assign them in the master group.
    server.drawdag("B-X-Y-Z-F D-F-G", &["G"]);
    assert!(client.output().is_empty());
    client.set_remote(&server);

    // To avoid reusing caches, reopen the graph from disk.
    client.reopen();

    assert_eq!(
        client.dump_state().await,
        r#"<spans [X:L+N0:N9, A:E+0:4]>
Lv0: RH0-4[], N0-N2[1], N3-N5[3, N2], N6-N9[1]
Lv1: N0-N5[1, 3]
P->C: 1->N0, 1->N6, 3->N3, N2->N3
0->A 1->B 2->C 3->D 4->E N0->X N1->Y N2->Z N3->F N4->G N5->H N6->I N7->J N8->K N9->L"#
    );

    // Force reassign of vertexes in the non-master group.
    client.flush("G").await;
    assert!(client.output().is_empty());

    //       I-J-K-L
    //      /
    //   A-B--C--D-E
    //      \     \
    //       X-Y-Z-F-G-H
    //
    // Check that:
    // - G % E = G+F+Z+Y+X are added to the master group.
    // - I::L remains in the non-master group unchanged.
    // - X::G are removed from the non-master group.
    // - H is re-inserted in the non-master group.
    assert_eq!(
        client.dump_state().await,
        r#"<spans [I:L+N6:N9, H+N0, A:G+0:9]>
Lv0: RH0-4[], 5-7[1], 8-9[3, 7], N0-N0[9], N6-N9[1]
Lv1: R0-4[]
P->C: 1->5, 1->N6, 3->8, 7->8, 9->N0
0->A 1->B 2->C 3->D 4->E 5->X 6->Y 7->Z 8->F 9->G N0->H N6->I N7->J N8->K N9->L"#
    );
}

#[tokio::test]
async fn test_resolve_misleading_merges() {
    // Test when the server graph gets more merges making vertexes
    // previously not a parent of a merge become a parent of a merge.

    // Initially, B is not a parent of a merge. A can be resolved as
    // C~2.
    let server1 = TestDag::draw("A-B-C # master: C");

    // Make B a parent of a merge by adding extra vertexes.
    // So A might be resolved as B~1 or C~2.
    let server2 = TestDag::draw(
        r#"
        A-B-C
           \
          D-E   # master: E C"#,
    );

    let client1 = server1.client_cloned_data().await.with_remote(&server2);

    // A is resolved to C~2, not B~1, since in the ancestors(C) sub-graph,
    // B is not a parent of a merge.
    client1.dag.vertex_id("A".into()).await.unwrap();
    assert_eq!(client1.output(), ["resolve names: [A], heads: [C]"]);
}

#[tokio::test]
async fn test_resolve_pick_path() {
    // Test when a vertex can be resolved in multiple ways, pick the one that is
    // supported by the client.

    // Make A resolve to D~1, and C~2.
    let server2 = TestDag::draw(
        r#"
           A-B-C
            \
             \ E
              \ \
               D-F 
                \
                 G-H-I   # master: F C I"#,
    );

    // The client does not have D:: part of the graph.
    let server1 = TestDag::draw("A-B-C # master: C");
    let client1 = server1.client_cloned_data().await.with_remote(&server2);

    // Can resolve A using B~1, not D~1.
    assert_eq!(client1.dag.vertex_id("A".into()).await.unwrap(), Id(0));
    assert_eq!(client1.output(), ["resolve names: [A], heads: [C]"]);

    // The client does not have B::, E:: part of the graph.
    // D is not considered as a parent of a merge in the graph.
    let server2 = TestDag::draw("A-D-G-H # master: H");
    let client2 = server2.client_cloned_data().await.with_remote(&server2);

    // Can resolve A using H~3, not B~1 or D~1 as requested.
    assert_eq!(client2.dag.vertex_id("A".into()).await.unwrap(), Id(0));
    assert_eq!(client2.output(), ["resolve names: [A], heads: [H]"]);
}

#[tokio::test]
async fn test_resolve_mixed_result() {
    // Test that Ok and Err can be both present in vertex_id_batch return value.
    let servers: Vec<_> = ["F J", "D J", "C E D F J"]
        .iter()
        .map(|master| {
            TestDag::draw(&format!(
                r#"
                    A-B-C-D-G-H-I-J
                       \   /
                        E-F   # master: {}"#,
                master
            ))
        })
        .collect();

    assert_eq!(
        servers
            .iter()
            .map(|s| s.debug_segments(0, Group::MASTER))
            .collect::<Vec<_>>()
            .join("\n"),
        r#"
        G+6 : J+9 [D+5, F+3] OnlyHead
        C+4 : D+5 [B+1]
        A+0 : F+3 [] Root OnlyHead

        G+6 : J+9 [D+3, F+5] OnlyHead
        E+4 : F+5 [B+1]
        A+0 : D+3 [] Root OnlyHead

        G+6 : J+9 [D+4, F+5] OnlyHead
        F+5 : F+5 [E+3]
        D+4 : D+4 [C+2]
        E+3 : E+3 [B+1]
        A+0 : C+2 [] Root OnlyHead"#
    );

    let names: Vec<_> = "A B C D E F G H I J X".split(' ').map(Into::into).collect();
    for server in servers {
        let client = TestDag::draw("A-B-C-D-G-H B-E-F-G # master: H")
            .client_cloned_data()
            .await
            .with_remote(&server);
        let ids = client.dag.vertex_id_batch(&names).await;
        assert_eq!(
            format!("{:?}", ids),
            "Ok([Ok(0), Ok(1), Ok(2), Ok(3), Ok(4), Ok(5), Ok(6), Ok(7), Err(VertexNotFound(I)), Err(VertexNotFound(J)), Err(VertexNotFound(X))])",
        );
        assert_eq!(
            client.output(),
            ["resolve names: [A, B, C, E, G, I, J, X], heads: [H]"]
        );
    }
}

#[tokio::test]
async fn test_flush_lazy_vertex() {
    // Test flushing with main vertex set to a lazy vertex.
    let server = TestDag::draw("A-B-C-D # master: D");
    let mut client = server.client_cloned_data().await;
    client.flush("B").await;
}

async fn client_for_local_cache_test() -> TestDag {
    let server = TestDag::draw("A-B-C-D-E-F-G # master: G");
    server.client_cloned_data().await
}

async fn check_local_cache(client: &TestDag, v: VertexName, id: Id) {
    // Try looking up vertex using different APIs.
    assert_eq!(client.dag.vertex_id(v.clone()).await.unwrap(), id);
    assert!(client.output().is_empty());

    assert!(client.dag.contains_vertex_name(&v.clone()).await.unwrap());
    assert!(client.output().is_empty());

    assert_eq!(
        client.dag.vertex_id_optional(&v.clone()).await.unwrap(),
        Some(id)
    );
    assert!(client.output().is_empty());

    assert_eq!(
        client
            .dag
            .vertex_id_with_max_group(&v.clone(), Group::MASTER)
            .await
            .unwrap(),
        Some(id)
    );
    assert!(client.output().is_empty());

    assert!(matches!(
        &client
            .dag
            .contains_vertex_name_locally(&[v.clone()])
            .await
            .unwrap()[..],
        [true]
    ));
    assert!(client.output().is_empty());

    assert!(matches!(
        &client.dag.vertex_id_batch(&[v.clone()]).await.unwrap()[..],
        [Ok(i)] if *i == id
    ));
    assert!(client.output().is_empty());

    // Try looking up Id using different APIs.
    assert_eq!(client.dag.vertex_name(id).await.unwrap(), v.clone());
    assert!(client.output().is_empty());

    assert!(matches!(
        &client.dag.contains_vertex_id_locally(&[id]).await.unwrap()[..],
        [true]
    ));
    assert!(client.output().is_empty());

    assert!(matches!(
        &client.dag.vertex_name_batch(&[id]).await.unwrap()[..],
        [Ok(n)] if n == &v
    ));
    assert!(client.output().is_empty());
}

#[tokio::test]
async fn test_local_cache_existing_vertex_to_id() {
    let client = client_for_local_cache_test().await;

    let v: VertexName = "C".into();
    let id = client.dag.vertex_id(v.clone()).await.unwrap();
    assert_eq!(client.output(), ["resolve names: [C], heads: [G]"]);

    check_local_cache(&client, v, id).await;
}

#[tokio::test]
async fn test_local_cache_existing_id_to_vertex() {
    let client = client_for_local_cache_test().await;

    let id = Id(3);
    let v = client.dag.vertex_name(id).await.unwrap();
    assert_eq!(client.output(), ["resolve paths: [G~3]"]);

    check_local_cache(&client, v, id).await;
}

#[tokio::test]
async fn test_local_cache_missing_vertex_to_id() {
    let client = client_for_local_cache_test().await;

    // Test that the local cache can prevent remote lookups resolving Vertex -> None.
    assert!(client.dag.vertex_id("Z".into()).await.is_err());
    assert_eq!(client.output(), ["resolve names: [Z], heads: [G]"]);

    // Try looking up using different APIs.
    assert!(client.dag.vertex_id("Z".into()).await.is_err());
    assert!(client.output().is_empty());

    assert!(!client.dag.contains_vertex_name(&"Z".into()).await.unwrap());
    assert!(client.output().is_empty());

    assert_eq!(
        client.dag.vertex_id_optional(&"Z".into()).await.unwrap(),
        None,
    );
    assert!(client.output().is_empty());

    assert_eq!(
        client
            .dag
            .vertex_id_with_max_group(&"Z".into(), Group::MASTER)
            .await
            .unwrap(),
        None,
    );
    assert!(client.output().is_empty());

    assert!(matches!(
        &client
            .dag
            .contains_vertex_name_locally(&["Z".into()])
            .await
            .unwrap()[..],
        [false]
    ));
    assert!(client.output().is_empty());

    assert!(matches!(
        &client.dag.vertex_id_batch(&["Z".into()]).await.unwrap()[..],
        [Err(_)]
    ));
    assert!(client.output().is_empty());
}
