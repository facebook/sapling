/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::TestDag;
use crate::ops::DagAlgorithm;
use crate::ops::DagExportCloneData;
use crate::ops::DagImportCloneData;
use crate::ops::IdConvert;
use crate::protocol;
use crate::protocol::RemoteIdConvertProtocol;
use crate::Id;
use crate::Result;
use crate::VertexName;
use futures::TryStreamExt;
use parking_lot::Mutex;
use std::sync::Arc;

struct ProtocolMonitor {
    inner: Box<dyn RemoteIdConvertProtocol>,
    output: Arc<Mutex<Vec<String>>>,
}

#[async_trait::async_trait]
impl RemoteIdConvertProtocol for ProtocolMonitor {
    async fn resolve_names_to_relative_paths(
        &self,
        heads: Vec<VertexName>,
        names: Vec<VertexName>,
    ) -> Result<Vec<(protocol::AncestorPath, Vec<VertexName>)>> {
        let msg = format!("resolve names: {:?}, heads: {:?}", &names, &heads);
        self.output.lock().push(msg);
        self.inner
            .resolve_names_to_relative_paths(heads, names)
            .await
    }

    async fn resolve_relative_paths_to_names(
        &self,
        paths: Vec<protocol::AncestorPath>,
    ) -> Result<Vec<(protocol::AncestorPath, Vec<VertexName>)>> {
        let msg = format!("resolve paths: {:?}", &paths);
        self.output.lock().push(msg);
        self.inner.resolve_relative_paths_to_names(paths).await
    }
}

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

    let data = server2.dag.export_clone_data().await.unwrap();
    let mut client = TestDag::new();
    client.dag.import_clone_data(data).await.unwrap();

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

    // Without remote protocol. Cannot solve id <-> names.
    assert!(client.dag.vertex_name(Id(9)).await.is_err());
    assert!(client.dag.vertex_id("A".into()).await.is_err());

    // With remote protocol. Be able to resolve id <-> names.
    let output = Arc::new(Default::default());
    let protocol = ProtocolMonitor {
        inner: Box::new(server1.dag),
        output: Arc::clone(&output),
    };
    client.dag.set_remote_protocol(Arc::new(protocol));
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
        output.lock().clone(),
        [
            "resolve paths: [D~1]",
            "resolve names: [E], heads: [M]",
            "resolve paths: [L~1, I~1, I~3, I~4]"
        ]
    );
}
