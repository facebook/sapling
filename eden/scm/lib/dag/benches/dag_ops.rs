/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use dag::{idmap::IdMap, segment::Dag, spanset::SpanSet, GroupId, Id};
use minibench::{bench, elapsed};
use tempfile::tempdir;

mod bindag;

fn main() {
    let parents = bindag::parse_bindag(bindag::MOZILLA);

    let head_name = format!("{}", parents.len() - 1).as_bytes().to_vec();
    let parents_by_name = |name: &[u8]| -> Result<Vec<Box<[u8]>>> {
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
    id_map
        .assign_head(&head_name, &parents_by_name, GroupId::MASTER)
        .unwrap();

    let head_id = id_map.find_id_by_slice(&head_name).unwrap().unwrap();
    let parents_by_id = id_map.build_get_parents_by_id(&parents_by_name);

    let dag_dir = tempdir().unwrap();

    bench("building segments", || {
        let mut dag = Dag::open(&dag_dir.path()).unwrap();
        elapsed(|| {
            dag.build_segments_volatile(head_id, &parents_by_id)
                .unwrap();
        })
    });

    // Write segments to filesystem.
    let mut dag = Dag::open(&dag_dir.path()).unwrap();
    let mut syncable = dag.prepare_filesystem_sync().unwrap();
    syncable
        .build_segments_persistent(head_id, &parents_by_id)
        .unwrap();
    syncable.sync(std::iter::once(&mut dag)).unwrap();

    let sample_two_ids: Vec<SpanSet> = (0..parents.len() as u64)
        .step_by(10079)
        .flat_map(|i| {
            (1..parents.len() as u64)
                .step_by(7919)
                .map(move |j| (Id(i), Id(j)).into())
        })
        .collect(); // 2679 samples
    let sample_sets: Vec<SpanSet> = (0..parents.len() as u64)
        .step_by(10079)
        .flat_map(|i| {
            ((i + 7919)..parents.len() as u64)
                .step_by(7919)
                .map(move |j| (Id(i)..=Id(j)).into())
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

    bench("descendants (small subset)", || {
        elapsed(|| {
            // "descendants" is extremely slow. Therefore only test a very
            // small subset.
            for set in sample_sets.iter().skip(500).take(2) {
                dag.descendants(set.clone()).unwrap();
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

    bench("parent_ids", || {
        elapsed(|| {
            for set in &sample_two_ids {
                for id in set.iter() {
                    dag.parent_ids(id).unwrap();
                }
            }
        })
    });

    bench("range (2 ids)", || {
        elapsed(|| {
            for set in &sample_two_ids {
                let ids: Vec<_> = set.iter().collect();
                dag.range(ids[0], ids[1]).unwrap();
            }
        })
    });

    bench("range (spans)", || {
        elapsed(|| {
            let mut iter = sample_sets.iter();
            if let (Some(set1), Some(set2)) = (iter.next(), iter.next_back()) {
                dag.range(set1.clone(), set2.clone()).unwrap();
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
