/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use dag::ops::DagImportCloneData;
use dag::ops::DagImportPullData;
use dag::CloneData;
use dag::Dag;
use dag::DagAlgorithm;
use dag::FlatSegment;
use dag::Group;
use dag::Id;
use dag::PreparedFlatSegments;
use dag::Vertex;
use dag::VertexListWithOptions;
use minibench::bench;
use minibench::elapsed;
use nonblocking::non_blocking_result as nbr;
use tempfile::tempdir;

pub fn main() {
    let clone_data = example_clone_data();
    let tip = clone_data.idmap.last_key_value().unwrap().1.clone();
    let expected_count = clone_data.flat_segments.vertex_count() as usize;

    let assert_graph_size = |dag: &Dag| {
        let actual_count = nbr(async { dag.all().await.unwrap().count().await }).unwrap();
        assert_eq!(actual_count, expected_count);
    };

    bench("dag_import/clone_clone_data", || {
        elapsed(|| {
            let _ = clone_data.clone();
        })
    });

    bench("dag_import/import_clone_data", || {
        let tmp = tempdir().unwrap();
        let mut dag = Dag::open(tmp.path()).unwrap();
        elapsed(|| {
            nbr(dag.import_clone_data(clone_data.clone())).unwrap();
            assert_graph_size(&dag);
        })
    });

    bench("dag_import/import_pull_data", || {
        let tmp = tempdir().unwrap();
        let mut dag = Dag::open(tmp.path()).unwrap();
        let heads =
            VertexListWithOptions::from(vec![tip.clone()]).with_highest_group(Group::MASTER);
        elapsed(|| {
            nbr(dag.import_pull_data(clone_data.clone(), &heads)).unwrap();
            assert_graph_size(&dag);
        })
    });
}

fn example_clone_data() -> CloneData<Vertex> {
    // To dump production clonedata for testing, run:
    //
    //   export CLONEDATA=/tmp/CLONEDATA
    //   sl dbsh -c "open(os.getenv('CLONEDATA'),'wb').write(b.cbor.dumps(api.pulllazy([],list(repo.nodes('master'))).export()))"
    if let Ok(path) = std::env::var("CLONEDATA") {
        eprintln!("Using CLONEDATA={}", &path);
        let data = std::fs::read(path).unwrap();
        let data: CloneData<Vertex> = serde_cbor::from_slice(&data).unwrap();
        return data;
    }

    let segments = bindag::parse_bindag_segments(bindag::MOZILLA);
    let segments = segments
        .into_iter()
        .map(|s| FlatSegment {
            low: Id(s.low as _),
            high: Id(s.high as _),
            parents: s.parents.as_ref().iter().map(|p| Id(*p as _)).collect(),
        })
        .collect();
    let flat_segments = PreparedFlatSegments { segments };

    let idmap = flat_segments
        .parents_head_and_roots()
        .into_iter()
        .map(|id| (id, id.to_string().into()))
        .collect();
    CloneData {
        flat_segments,
        idmap,
    }
}
