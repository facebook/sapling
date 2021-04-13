/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::nameset::SyncNameSetQuery;
use crate::ops::DagAddHeads;
use crate::ops::DagAlgorithm;
use crate::ops::DagExportCloneData;
use crate::ops::DagImportCloneData;
use crate::ops::DagPersistent;
use crate::ops::IdConvert;
use crate::protocol;
use crate::protocol::RemoteIdConvertProtocol;
use crate::render::render_namedag;
use crate::NameDag;
use crate::Result;
use crate::Vertex;
use nonblocking::non_blocking_result;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

/// Dag structure for testing purpose.
pub struct TestDag {
    pub dag: NameDag,
    pub seg_size: usize,
    pub dir: tempfile::TempDir,
    pub output: Arc<Mutex<Vec<String>>>,
}

impl TestDag {
    /// Creates a `TestDag` for testing.
    /// Side effect of the `TestDag` will be removed on drop.
    pub fn new() -> Self {
        Self::new_with_segment_size(3)
    }

    /// Creates a `TestDag` with a specific segment size.
    pub fn new_with_segment_size(seg_size: usize) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let dag = NameDag::open(dir.path().join("n")).unwrap();
        Self {
            dir,
            dag,
            seg_size,
            output: Default::default(),
        }
    }

    /// Add vertexes to the graph.
    pub fn drawdag(&mut self, text: &str, master_heads: &[&str]) {
        self.drawdag_with_limited_heads(text, master_heads, None);
    }

    /// Add vertexes to the graph.
    ///
    /// If `heads` is set, ignore part of the graph. Only consider specified
    /// heads.
    pub fn drawdag_with_limited_heads(
        &mut self,
        text: &str,
        master_heads: &[&str],
        heads: Option<&[&str]>,
    ) {
        let (all_heads, parent_func) = get_heads_and_parents_func_from_ascii(text);
        let heads = match heads {
            Some(heads) => heads
                .iter()
                .map(|s| Vertex::copy_from(s.as_bytes()))
                .collect(),
            None => all_heads,
        };
        self.dag.dag.set_new_segment_size(self.seg_size);
        non_blocking_result(self.dag.add_heads(&parent_func, &heads)).unwrap();
        self.validate();
        let master_heads = master_heads
            .iter()
            .map(|s| Vertex::copy_from(s.as_bytes()))
            .collect::<Vec<_>>();
        let need_flush = !master_heads.is_empty();
        if need_flush {
            non_blocking_result(self.dag.flush(&master_heads)).unwrap();
        }
        self.validate();
    }

    /// Replace ASCII with Ids in the graph.
    pub fn annotate_ascii(&self, text: &str) -> String {
        self.dag.map.replace(text)
    }

    /// Render the segments.
    pub fn render_segments(&self) -> String {
        format!("{:?}", &self.dag.dag)
    }

    /// Render the graph.
    pub fn render_graph(&self) -> String {
        render_namedag(&self.dag, |v| {
            Some(
                non_blocking_result(self.dag.vertex_id(v.clone()))
                    .unwrap()
                    .to_string(),
            )
        })
        .unwrap()
    }

    /// Use this DAG as the "server", return the "client" Dag that has lazy Vertexes.
    pub async fn client(&self) -> TestDag {
        let data = self.dag.export_clone_data().await.unwrap();
        let mut client = TestDag::new();
        client.dag.import_clone_data(data).await.unwrap();
        let remote = self.remote_protocol(client.output.clone());
        client.dag.set_remote_protocol(remote);
        client
    }

    /// Remote protocol used to resolve Id <-> Vertex remotely using the test dag
    /// as the "server".
    ///
    /// Logs of the remote access will be written to `output`.
    pub fn remote_protocol(
        &self,
        output: Arc<Mutex<Vec<String>>>,
    ) -> Arc<dyn RemoteIdConvertProtocol> {
        let remote = ProtocolMonitor {
            inner: Box::new(self.dag.try_snapshot().unwrap()),
            output,
        };
        Arc::new(remote)
    }

    /// Output of remote protocols since the last call.
    pub fn output(&self) -> Vec<String> {
        let mut result = Vec::new();
        let mut output = self.output.lock();
        std::mem::swap(&mut result, &mut *output);
        result
    }

    fn validate(&self) {
        // All vertexes should be accessible, and round-trip through IdMap.
        for v in non_blocking_result(self.dag.all()).unwrap().iter().unwrap() {
            let v = v.unwrap();
            let id = non_blocking_result(self.dag.vertex_id(v.clone())).unwrap();
            let v2 = non_blocking_result(self.dag.vertex_name(id)).unwrap();
            assert_eq!(v, v2);
        }
    }
}

struct ProtocolMonitor {
    inner: Box<dyn RemoteIdConvertProtocol>,
    output: Arc<Mutex<Vec<String>>>,
}

#[async_trait::async_trait]
impl RemoteIdConvertProtocol for ProtocolMonitor {
    async fn resolve_names_to_relative_paths(
        &self,
        heads: Vec<Vertex>,
        names: Vec<Vertex>,
    ) -> Result<Vec<(protocol::AncestorPath, Vec<Vertex>)>> {
        let msg = format!("resolve names: {:?}, heads: {:?}", &names, &heads);
        self.output.lock().push(msg);
        self.inner
            .resolve_names_to_relative_paths(heads, names)
            .await
    }

    async fn resolve_relative_paths_to_names(
        &self,
        paths: Vec<protocol::AncestorPath>,
    ) -> Result<Vec<(protocol::AncestorPath, Vec<Vertex>)>> {
        let msg = format!("resolve paths: {:?}", &paths);
        self.output.lock().push(msg);
        self.inner.resolve_relative_paths_to_names(paths).await
    }
}

fn get_heads_and_parents_func_from_ascii(
    text: &str,
) -> (Vec<Vertex>, HashMap<Vertex, Vec<Vertex>>) {
    let parents = drawdag::parse(&text);
    let mut heads = parents
        .keys()
        .collect::<HashSet<_>>()
        .difference(&parents.values().flat_map(|ps| ps.into_iter()).collect())
        .map(|&v| Vertex::copy_from(v.as_bytes()))
        .collect::<Vec<_>>();
    heads.sort();
    let v = |s: String| Vertex::copy_from(s.as_bytes());
    let parents = parents
        .into_iter()
        .map(|(k, vs)| (v(k), vs.into_iter().map(v).collect()))
        .collect();
    (heads, parents)
}
