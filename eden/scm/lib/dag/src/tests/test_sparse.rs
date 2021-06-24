/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::ProtocolMonitor;
use super::TestDag;
use crate::ops::DagAlgorithm;
use crate::ops::DagPersistent;
use crate::ops::IdConvert;
use crate::ops::{DagAddHeads, DagImportPullData, DagPullFastForwardMasterData};
use crate::Group;
use crate::Id;
use crate::VertexName;
use futures::TryStreamExt;
use std::sync::Arc;

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
   Next Free Id: 13
   Segments: 6
    11 : M+12 [D+10, J+7] OnlyHead
    9 : D+10 [B+8, G+2]
    B+8 : B+8 [0]
    J+7 : J+7 [I+4, L+6] OnlyHead
    5 : L+6 [] Root
    0 : I+4 [] Root OnlyHead
  Group Non-Master:
   Next Free Id: N0
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
                "resolve paths: [L~1, I~1, I~3(+2)]"
            ]
        );
    }
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
        .add_heads(&parents, &["G".into(), "K".into()])
        .await
        .unwrap();
    assert_eq!(
        client.output(),
        [
            "resolve names: [K, I, G, F, A], heads: [B]",
            "resolve names: [H, D, C], heads: [B]"
        ]
    );

    client.dag.flush(&["G".into()]).await.unwrap();
    assert_eq!(client.output(), ["resolve names: [K, I, G, C], heads: [B]"]);

    let mut client = server.client_cloned_data().await;
    client
        .dag
        .add_heads_and_flush(&parents, &["K".into()], &["G".into()])
        .await
        .unwrap();
    assert_eq!(
        client.output(),
        [
            "resolve names: [G, K, I, H, A], heads: [B]",
            "resolve names: [F, D, C], heads: [B]"
        ]
    );
}

#[tokio::test]
async fn test_basic_pull() {
    let server = TestDag::draw("A-B-C-D  # master: D");
    let mut client = server.client().await;
    client.drawdag("A-B", &["B"]);

    let pull_data = server
        .dag
        .pull_fast_forward_master(VertexName("B".into()), VertexName("D".into()))
        .await
        .unwrap();

    client.dag.import_pull_data(pull_data).await.unwrap();

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
        vec![
            "resolve names: [E, B, A], heads: []".to_string(),
            "resolve names: [E, B, A], heads: []".to_string(),
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

    // BUG: C-D appear twice in the graph.
    client.pull_ff_master(&server, "B", "F").await.unwrap();
    assert_eq!(
        client.render_graph(),
        r#"
            F  7
            │
            E  6
            │
            D  3
            │
            C  4
            │
            │ D  3
            │ │
            │ C  4
            ├─╯
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
    assert_eq!(client.output(), [] as [&str; 0]);

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
        I+13 : K+15 [D+5, H+12, M+9]
        G+11 : H+12 [F+7, N+10]
        L+8 : N+10 [C+4]
        B+3 : F+7 [A+0]
        X+1 : Y+2 [] Root
        A+0 : A+0 [] Root OnlyHead"#
    );


    // BUG: Error out with VertexNotFound(C)
    client.pull_ff_master(&server, "E", "K").await.unwrap();
}
