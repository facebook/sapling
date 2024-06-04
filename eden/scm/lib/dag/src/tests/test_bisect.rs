/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;

use futures::TryStreamExt;
use nonblocking::non_blocking_result as r;

use crate::ops::DagAlgorithm;
use crate::tests::TestDag;
use crate::Set;
use crate::Vertex;

impl TestDag {
    /// For each vertex, set it as "first bad" and check how many steps bisect takes.
    /// Returns a human-readable multi-line report.
    fn bisect_step_distribution(&self) -> Vec<String> {
        let all = r(self.dag.all()).unwrap();

        let iter = r(all.iter()).unwrap();
        let all_vec: Vec<Vertex> = r(iter.try_collect()).unwrap();

        let mut step_to_vec = BTreeMap::<usize, Vec<Vertex>>::new();

        for first_bad in all_vec {
            let all_bad = r(self.dag.descendants(first_bad.clone().into())).unwrap();
            let all_good = all.difference(&all_bad);
            if r(all_good.count()).unwrap() == 0 {
                continue;
            }
            let skip = Set::empty();

            // Known, current good and bad
            let mut good = r(self.dag.roots(all_good.clone())).unwrap();
            let mut bad = r(self.dag.heads(all_bad)).unwrap();

            let mut step = 0;
            loop {
                let (test_vertex, _untested, _heads) =
                    r(self
                        .dag
                        .suggest_bisect(good.clone(), bad.clone(), skip.clone()))
                    .unwrap();
                match test_vertex {
                    None => {
                        step_to_vec.entry(step).or_default().push(first_bad.clone());
                        break;
                    }
                    Some(v) => {
                        step += 1;
                        if r(all_good.contains(&v)).unwrap() {
                            good = good.union(&r(self.dag.sort(&v.into())).unwrap());
                        } else {
                            bad = bad.union(&r(self.dag.sort(&v.into())).unwrap());
                        }
                    }
                }
            }
        }

        let mut lines = vec!["Step | Vertexes".to_string()];
        let mut total = 0;
        let mut count = 0;
        for (step, mut vertexes) in step_to_vec {
            vertexes.sort_unstable();
            let len = vertexes.len();
            total += len * step;
            count += len;
            lines.push(format!("{:4} |{:3}: {:?}", step, len, vertexes));
        }
        let avg = total as f32 / count as f32;
        lines.push(format!("Average: {:.2}", avg));
        lines
    }
}

#[test]
fn test_linear_step_distribution() {
    let dag = TestDag::draw("A01..A20");
    assert_eq!(
        dag.bisect_step_distribution(),
        [
            "Step | Vertexes",
            "   4 | 13: [A02, A03, A04, A05, A06, A07, A08, A11, A12, A13, A16, A17, A18]",
            "   5 |  6: [A09, A10, A14, A15, A19, A20]",
            "Average: 4.32"
        ]
    );
}

#[test]
fn test_merge1_step_distribution() {
    let dag = TestDag::draw(
        r#"
           Y
          / \
        A10 B20
         :   :
        A01 B01
          \ /
           X  "#,
    );
    assert_eq!(
        dag.bisect_step_distribution(),
        [
            "Step | Vertexes",
            "   4 |  1: [B01]",
            "   5 | 30: [A01, A02, A03, A04, A05, A06, A07, A08, A09, A10, B02, B03, B04, B05, B06, B07, B08, B09, B10, B11, B12, B13, B14, B15, B16, B17, B18, B19, B20, Y]",
            "Average: 4.97"
        ]
    );
}

#[test]
fn test_merge2_step_distribution() {
    let dag = TestDag::draw(
        r#"
            F20
             :
            F10
             : \
            F01 \
            /    |
          E20    |
           :     |
          E10   D20
           : \   :
           :  \  :
          E01  \ :
          / \   D10
        B20 C20  :
         :   :   :
        B01 C01 D01
          \ /  /
          A20 /
           : /
          A10
           :
          A01  "#,
    );
    assert_eq!(
        dag.bisect_step_distribution(),
        [
            "Step | Vertexes",
            "   6 | 16: [A02, A17, B12, C01, D01, D02, D03, D06, D07, D08, D11, D12, D13, D16, D17, D18]",
            "   7 | 89: [A03, A04, A05, A06, A07, A08, A09, A10, A11, A12, A13, A14, A15, A16, A18, A19, A20, B01, B02, B03, B04, B05, B06, B07, B08, B09, B10, B11, B13, B14, B15, B16, B17, B18, B19, B20, C02, C03, C04, C05, C06, C07, C08, C09, C10, C11, C12, C13, C14, C15, C16, C17, C18, C19, C20, D04, D05, D09, D10, D14, D15, D19, D20, E01, E02, E03, E04, E05, E06, E07, E08, E11, E12, E13, E16, E17, E18, F01, F02, F03, F06, F07, F08, F11, F12, F13, F16, F17, F18]",
            "   8 | 14: [E09, E10, E14, E15, E19, E20, F04, F05, F09, F10, F14, F15, F19, F20]",
            "Average: 6.98"
        ]
    );
}
