/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use dag_types::Group;
use nonblocking::non_blocking_result as r;

use crate::ops::DagAddHeads;
use crate::ops::IdConvert;
use crate::tests::DrawDag;
use crate::tests::TestDag;
use crate::Result;
use crate::VertexOptions;

#[test]
fn test_add_heads_refuse_reassign_group() -> Result<()> {
    // Try to reassign "C" from NON_MASTER to MASTER group. It should error out.
    let mut dag = TestDag::draw("A..C");

    // "C" is in the NON_MASTER group now.
    assert_eq!(r(dag.vertex_id("C".into()))?.group(), Group::NON_MASTER);

    // Explicitly reassigning "C" is an error.
    let parents = DrawDag::from("A..E");
    let opts = VertexOptions {
        desired_group: Group::MASTER,
        ..Default::default()
    };
    let err = r(dag.add_heads(&parents, &vec![("C".into(), opts.clone())].into())).unwrap_err();
    assert_eq!(
        err.to_string(),
        "ProgrammingError: add_heads: cannot re-assign C:N2 from Group Non-Master to Group Master (desired), use add_heads_and_flush instead"
    );

    // Implicitly reassigning "C" is also an error.
    let err = r(dag.add_heads(&parents, &vec![("E".into(), opts.clone())].into())).unwrap_err();
    assert_eq!(
        err.to_string(),
        "bug: new entry 0 = [65] conflicts with an existing entry N0 = [65]"
    );

    // After the error, no "E" or "D" was actually added.
    assert!(!r(dag.contains_vertex_name(&"D".into()))?);
    assert!(!r(dag.contains_vertex_name(&"E".into()))?);

    Ok(())
}
