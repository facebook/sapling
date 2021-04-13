/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::TestDag;
use crate::ops::DagAlgorithm;
use crate::ops::IdConvert;
use crate::Id;
use crate::VertexName;
use futures::TryStreamExt;

#[tokio::test]
async fn test_sparse_dag() {
    // In this test, we have 3 dags:
    // - server1: A complete dag. Emulates server-side.
    // - server2: An isomorphism of server1. But uses different ids.
    // - client3: A sparse dag. Emulates client-side. Cloned from server2.
    //   Speaks remote protocol with server1.
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

    let client = server2.client().await;

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

#[tokio::test]
async fn test_negative_cache() {
    let server = TestDag::draw("A-B  # master: B");

    let mut client = server.client().await;

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
