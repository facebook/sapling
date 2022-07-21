/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use dag::idmap::IdMap;
use dag::idmap::IdMapAssignHead;
use dag::Group;
use dag::IdDag;
use dag::IdSet;
use dag::InProcessIdDag;
use dag::VertexName;
use minibench::bench;
use minibench::elapsed;
use nonblocking::non_blocking_result as nbr;
use tempfile::tempdir;

type ParentsFunc<'a> = Box<dyn Fn(VertexName) -> dag::Result<Vec<VertexName>> + Send + Sync + 'a>;

pub fn main() {
    println!(
        "benchmarking {} serde",
        std::any::type_name::<InProcessIdDag>()
    );
    let parents = bindag::parse_bindag(bindag::MOZILLA);

    let head_name = VertexName::copy_from(format!("{}", parents.len() - 1).as_bytes());
    let parents_by_name: ParentsFunc =
        Box::new(|name: VertexName| -> dag::Result<Vec<VertexName>> {
            let i = String::from_utf8(name.as_ref().to_vec())
                .unwrap()
                .parse::<usize>()
                .unwrap();
            Ok(parents[i]
                .iter()
                .map(|p| format!("{}", p).as_bytes().to_vec().into())
                .collect())
        });

    let id_map_dir = tempdir().unwrap();
    let mut id_map = IdMap::open(id_map_dir.path()).unwrap();
    let mut covered_ids = IdSet::empty();
    let reserved_ids = IdSet::empty();
    let outcome = nbr(id_map.assign_head(
        head_name.clone(),
        &parents_by_name,
        Group::MASTER,
        &mut covered_ids,
        &reserved_ids,
    ))
    .unwrap();
    let mut iddag = IdDag::new_in_process();
    iddag
        .build_segments_from_prepared_flat_segments(&outcome)
        .unwrap();

    let mut blob = Vec::new();
    bench("serializing inprocess iddag with mincode", || {
        elapsed(|| {
            blob = mincode::serialize(&iddag).unwrap();
        })
    });

    println!("mincode serialized blob has {} bytes", blob.len());

    bench("deserializing inprocess iddag with mincode", || {
        elapsed(|| {
            let _new_iddag: InProcessIdDag = mincode::deserialize(&blob).unwrap();
        })
    });
}
