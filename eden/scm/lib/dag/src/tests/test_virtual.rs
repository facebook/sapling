/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use dag_types::CloneData;
use dag_types::Group;
use nonblocking::non_blocking as nb;
use nonblocking::non_blocking_result as r;

use crate::ops::DagAddHeads;
use crate::ops::DagPersistent;
use crate::ops::IdConvert;
use crate::tests::DrawDag;
use crate::tests::TestDag;
use crate::DagAlgorithm;
use crate::Result;
use crate::VertexListWithOptions;
use crate::VertexOptions;

impl TestDag {
    fn insert_virtual(&mut self, ascii: &str) {
        let parents = DrawDag::from(ascii);
        let opts = VertexOptions {
            desired_group: Group::VIRTUAL,
            ..Default::default()
        };
        let heads = parents
            .heads()
            .into_iter()
            .map(|h| (h, opts.clone()))
            .collect::<Vec<_>>();
        r(self.add_heads(&parents, &heads.into())).unwrap();
    }
}

#[test]
fn test_virtual_group_preserve_verlinks() -> Result<()> {
    let mut dag = TestDag::draw("A..C");
    let dag_v1 = dag.dag_version().clone();
    let map_v1 = dag.map_version().clone();

    // Insert (D, E) to the virtual group.
    dag.insert_virtual("C..E");
    let c_id = r(dag.vertex_id("C".into()))?;
    let d_id = r(dag.vertex_id("D".into()))?;
    let e_id = r(dag.vertex_id("E".into()))?;
    assert!(!c_id.is_virtual());
    assert!(d_id.is_virtual());
    assert!(e_id.is_virtual());

    // The verlinks are not bumped for now.
    // This is a trade-off of avoiding losing fast paths.
    let dag_v2 = dag.dag_version().clone();
    let map_v2 = dag.map_version().clone();
    assert_eq!(dag_v2, dag_v1);
    assert_eq!(map_v2, map_v1);
    Ok(())
}

#[test]
fn test_virtual_group_can_be_queried() -> Result<()> {
    let mut dag = TestDag::draw("A..C");
    dag.insert_virtual("C..E");
    let ancestors = r(dag.ancestors("D".into()))?;
    assert_eq!(format!("{:?}", ancestors), "<spans [D+V0, A:C+N0:N2]>");
    Ok(())
}

#[test]
fn test_virtual_group_reinsert_is_noop() -> Result<()> {
    let mut dag = TestDag::draw("A");
    dag.insert_virtual("A-B");
    let b_id = r(dag.vertex_id("B".into()))?;
    dag.insert_virtual("A-B");
    let b_id2 = r(dag.vertex_id("B".into()))?;
    assert_eq!(b_id, b_id2);
    Ok(())
}

#[test]
fn test_virtual_group_does_not_block_write_operations() -> Result<()> {
    let mut dag = TestDag::draw("A..C");
    dag.insert_virtual("C..E");

    // TODO: flush() should attempt to preserve virtual vertexes.
    nb(dag.flush("B"))?;
    assert!(!r(dag.contains_vertex_name(&"E".into()))?);

    // TODO: add_heads_and_flush() should attempt to preserve virtual vertexes.
    let parents = DrawDag::from("");
    r(dag.add_heads_and_flush(&parents, &VertexListWithOptions::default()))?;
    assert!(!r(dag.contains_vertex_name(&"E".into()))?);

    // TODO: import_pull_data() should attempt to preserve virtual vertexes.
    let data = CloneData {
        flat_segments: Default::default(),
        idmap: Default::default(),
    };
    r(dag.import_pull_data(data, VertexListWithOptions::default()))?;
    assert!(!r(dag.contains_vertex_name(&"E".into()))?);

    Ok(())
}
