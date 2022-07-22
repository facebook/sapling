/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use super::TestDag;
use crate::ops::DagAlgorithm;
use crate::ops::Parents;
use crate::Set;
use crate::Vertex;

#[tokio::test]
async fn test_to_parents() {
    let dag = TestDag::draw(
        r#"
        A---B---C---D---E
              /  \     /
         F---G    H---I"#,
    );

    assert_eq!(dump_to_parents("A E", &dag).await, "A -> []; E -> [A]");
    assert_eq!(dump_to_parents("A B", &dag).await, "A -> []; B -> [A]");
    assert_eq!(
        dump_to_parents("A B C D E", &dag).await,
        "A -> []; B -> [A]; C -> [B]; D -> [C]; E -> [D]"
    );
    assert_eq!(
        dump_to_parents("A G D E", &dag).await,
        "A -> []; G -> []; D -> [G, A]; E -> [D]"
    );
    assert_eq!(
        dump_to_parents("A G C I E", &dag).await,
        "A -> []; G -> []; C -> [G, A]; I -> [C]; E -> [I]"
    );
    assert_eq!(
        dump_to_parents("C H E", &dag).await,
        "C -> []; H -> [C]; E -> [H]"
    );
    assert_eq!(
        dump_to_parents("C D H E", &dag).await,
        "C -> []; D -> [C]; H -> [C]; E -> [H, D]"
    );
    assert_eq!(
        dump_to_parents("C D I E", &dag).await,
        "C -> []; D -> [C]; I -> [C]; E -> [D, I]"
    );
    assert_eq!(
        dump_to_parents("A B C D E F G H I", &dag).await,
        "A -> []; B -> [A]; C -> [B, G]; D -> [C]; E -> [D, I]; F -> []; G -> [F]; H -> [C]; I -> [H]"
    );
}

/// Dump parents information for given set (separated by space) as a string.
async fn dump_to_parents(names: &'static str, dag: &TestDag) -> String {
    let names: Vec<Vertex> = names.split(' ').map(Into::into).collect();
    let set = Set::from_static_names(names.clone());
    assert!(set.to_parents().await.unwrap().is_none());
    let set = dag.dag.sort(&set).await.unwrap();
    let parents = set.to_parents().await.unwrap().unwrap();
    let mut output = Vec::new();
    for name in names {
        let parents = parents.parent_names(name.clone()).await.unwrap();
        output.push(format!("{:?} -> {:?}", name, parents));
    }
    output.join("; ")
}
