// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use dag::{idmap::IdMap, segment::Dag, spanset::SpanSet};
use failure::Fallible;
use minibench::{bench, elapsed};
use tempfile::tempdir;

mod bindag;

fn main() {
    let parents = bindag::parse_bindag(bindag::MOZILLA);

    let head_name = format!("{}", parents.len() - 1).as_bytes().to_vec();
    let parents_by_name = |name: &[u8]| -> Fallible<Vec<Box<[u8]>>> {
        let i = String::from_utf8(name.to_vec())
            .unwrap()
            .parse::<usize>()
            .unwrap();
        Ok(parents[i]
            .iter()
            .map(|p| format!("{}", p).as_bytes().to_vec().into())
            .collect())
    };

    let id_map_dir = tempdir().unwrap();
    let mut id_map = IdMap::open(id_map_dir.path()).unwrap();
    id_map.assign_head(&head_name, &parents_by_name).unwrap();

    let head_id = id_map.find_id_by_slice(&head_name).unwrap().unwrap();
    let parents_by_id = id_map.build_get_parents_by_id(&parents_by_name);

    let segment_size = 16;
    let dag_dir = tempdir().unwrap();

    bench("building segments", || {
        let mut dag = Dag::open(&dag_dir.path()).unwrap();
        elapsed(|| {
            dag.build_flat_segments(head_id, &parents_by_id, 0).unwrap();
            for level in 1..7 {
                let segment_len = dag
                    .build_high_level_segments(level, segment_size, false)
                    .unwrap();
                if segment_len == 0 {
                    break;
                }
            }
        })
    });

    // Write segments to filesystem.
    let mut dag = Dag::open(&dag_dir.path()).unwrap();
    {
        let mut dag = dag.prepare_filesystem_sync().unwrap();
        dag.build_flat_segments(head_id, &parents_by_id, 0).unwrap();
        for level in 1.. {
            let segment_len = dag
                .build_high_level_segments(level, segment_size, true)
                .unwrap();
            if segment_len == 0 {
                break;
            }
        }
        dag.sync().unwrap();
    }

    let sample_two_ids: Vec<SpanSet> = (0..parents.len() as u64)
        .step_by(10079)
        .flat_map(|i| {
            (1..parents.len() as u64)
                .step_by(7919)
                .map(move |j| (i, j).into())
        })
        .collect(); // 2679 samples
    let sample_sets: Vec<SpanSet> = (0..parents.len() as u64)
        .step_by(10079)
        .flat_map(|i| {
            ((i + 7919)..parents.len() as u64)
                .step_by(7919)
                .map(move |j| (i..=j).into())
        })
        .collect(); // 1471 samples

    bench("ancestors", || {
        elapsed(|| {
            for set in &sample_sets {
                dag.ancestors(set.clone()).unwrap();
            }
        })
    });

    bench("children", || {
        elapsed(|| {
            for set in &sample_sets {
                dag.children(set.clone()).unwrap();
            }
        })
    });

    bench("common_ancestors (spans)", || {
        elapsed(|| {
            for set in &sample_sets {
                dag.common_ancestors(set.clone()).unwrap();
            }
        })
    });

    bench("gca_one (2 ids)", || {
        elapsed(|| {
            for set in &sample_two_ids {
                dag.gca_one(set.clone()).unwrap();
            }
        })
    });

    bench("gca_one (spans)", || {
        elapsed(|| {
            for set in &sample_sets {
                dag.gca_one(set.clone()).unwrap();
            }
        })
    });

    bench("gca_all (2 ids)", || {
        elapsed(|| {
            for set in &sample_two_ids {
                dag.gca_all(set.clone()).unwrap();
            }
        })
    });

    bench("gca_all (spans)", || {
        elapsed(|| {
            for set in &sample_sets {
                dag.gca_all(set.clone()).unwrap();
            }
        })
    });

    bench("heads", || {
        elapsed(|| {
            for set in &sample_sets {
                dag.heads(set.clone()).unwrap();
            }
        })
    });

    bench("heads_ancestors", || {
        elapsed(|| {
            for set in &sample_sets {
                dag.heads_ancestors(set.clone()).unwrap();
            }
        })
    });

    bench("is_ancestor", || {
        elapsed(|| {
            for set in &sample_two_ids {
                let ids: Vec<_> = set.iter().collect();
                dag.is_ancestor(ids[0], ids[1]).unwrap();
            }
        })
    });

    bench("parents", || {
        elapsed(|| {
            for set in &sample_sets {
                dag.parents(set.clone()).unwrap();
            }
        })
    });

    bench("roots", || {
        elapsed(|| {
            for set in &sample_sets {
                dag.roots(set.clone()).unwrap();
            }
        })
    });
}
