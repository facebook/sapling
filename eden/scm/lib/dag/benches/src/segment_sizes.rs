/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use dag::idmap::IdMap;
use dag::idmap::IdMapAssignHead;
use dag::ops::Persist;
use dag::Group;
use dag::Id;
use dag::IdDag;
use dag::IdSet;
use dag::VertexName;
use minibench::bench;
use minibench::elapsed;
use nonblocking::non_blocking_result as nbr;
use tempfile::tempdir;

type ParentsFunc<'a> = Box<dyn Fn(VertexName) -> dag::Result<Vec<VertexName>> + Send + Sync + 'a>;

pub fn main() {
    let parents = bindag::parse_bindag(bindag::MOZILLA);

    let head_name = VertexName::copy_from(format!("{}", parents.len() - 1).as_bytes());
    let parents_by_name = |name: VertexName| -> dag::Result<Vec<VertexName>> {
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
    let mut covered_ids = IdSet::empty();
    let reserved_ids = IdSet::empty();
    let prepared_segments = nbr(id_map.assign_head(
        head_name,
        &(Box::new(parents_by_name) as ParentsFunc),
        Group::MASTER,
        &mut covered_ids,
        &reserved_ids,
    ))
    .unwrap();

    // Test the size, and generation speed, and ancestor calcuation speed
    // with some different segment sizes.
    for segment_size in [4, 8, 10, 12, 14, 16, 18, 20, 22, 24, 32, 64, 128] {
        let dag_dir = tempdir().unwrap();
        let mut dag = IdDag::open(&dag_dir.path()).unwrap();
        dag.set_new_segment_size(segment_size);
        let segment_len = dag
            .build_segments_from_prepared_flat_segments(&prepared_segments)
            .unwrap();
        {
            let locked = dag.lock().unwrap();
            dag.persist(&locked).unwrap();
        }

        let log_len = dag_dir.path().join("log").metadata().unwrap().len();
        eprintln!("segments: {}  log len: {}", segment_len, log_len);

        bench(
            format!("ancestor calcuation segment_size={}", segment_size),
            || {
                let dag = IdDag::open(&dag_dir.path()).unwrap();
                elapsed(|| {
                    for i in (0..parents.len() as u64).step_by(10079) {
                        for j in (1..parents.len() as u64).step_by(2351) {
                            dag.gca_one((Id(i), Id(j)).into()).unwrap();
                        }
                    }
                })
            },
        );
    }
}
