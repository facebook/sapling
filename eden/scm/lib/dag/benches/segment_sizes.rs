/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use dag::{idmap::IdMap, segment::Dag, Group, Id, VertexName};
use minibench::{
    bench, elapsed,
    measure::{self, Measure},
};
use tempfile::tempdir;

mod bindag;

fn main() {
    let parents = bindag::parse_bindag(bindag::MOZILLA);

    let head_name = VertexName::copy_from(format!("{}", parents.len() - 1).as_bytes());
    let parents_by_name = |name: VertexName| -> Result<Vec<VertexName>> {
        let i = String::from_utf8(name.as_ref().to_vec())
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
        .assign_head(head_name.clone(), &parents_by_name, Group::MASTER)
        .unwrap();

    let head_id = id_map.find_id_by_name(head_name.as_ref()).unwrap().unwrap();
    let parents_by_id = id_map.build_get_parents_by_id(&parents_by_name);

    // Test the size, and generation speed, and ancestor calcuation speed
    // with some different segment sizes.
    for &segment_size in [4, 8, 10, 12, 14, 16, 18, 20, 22, 24, 32, 64, 128].iter() {
        let dag_dir = tempdir().unwrap();
        let mut built = false;
        bench(format!("building segment_size={}", segment_size), || {
            built = true;
            measure::Both::<measure::WallClock, String>::measure(|| {
                let mut dag = Dag::open(&dag_dir.path()).unwrap();
                dag.set_new_segment_size(segment_size);
                let mut syncable = dag.prepare_filesystem_sync().unwrap();
                let segment_len = syncable
                    .build_segments_persistent(head_id, &parents_by_id)
                    .unwrap();
                syncable.sync(std::iter::once(&mut dag)).unwrap();
                let log_len = dag_dir.path().join("log").metadata().unwrap().len();
                format!("segments: {}  log len: {}", segment_len, log_len)
            })
        });

        bench(
            format!("ancestor calcuation segment_size={}", segment_size),
            || {
                assert!(built, "segments must be built to run this benchmak");
                let dag = Dag::open(&dag_dir.path()).unwrap();
                elapsed(|| {
                    for i in (0..parents.len() as u64).step_by(10079) {
                        for j in (1..parents.len() as u64).step_by(2351) {
                            dag.gca_one((Id(i), Id(j))).unwrap();
                        }
                    }
                })
            },
        );
    }
}
