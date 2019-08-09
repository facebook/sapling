// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use dag::{idmap::IdMap, segment::Dag};
use failure::Fallible;
use minibench::{
    bench, elapsed,
    measure::{self, Measure},
};
use tempfile::tempdir;
use vlqencoding::VLQDecode;

static BINDAG_MOZILLA: &[u8] = include_bytes!("mozilla-central.bindag");

fn parse_bindag(bindag: &[u8]) -> Vec<Vec<usize>> {
    let mut parents = Vec::new();
    let mut cur = std::io::Cursor::new(bindag);
    let mut read_next = move || -> Result<usize, _> { cur.read_vlq() };

    while let Ok(i) = read_next() {
        let next_id = parents.len();
        match i {
            0 => {
                // no parents
                parents.push(vec![]);
            }
            1 => {
                // 1 specified parent
                let p1 = next_id - read_next().unwrap() - 1;
                parents.push(vec![p1]);
            }
            2 => {
                // 2 specified parents
                let p1 = next_id - read_next().unwrap() - 1;
                let p2 = next_id - read_next().unwrap() - 1;
                parents.push(vec![p1, p2]);
            }
            3 => {
                // 2 parents, p2 specified
                let p1 = next_id - 1;
                let p2 = next_id - read_next().unwrap() - 1;
                parents.push(vec![p1, p2]);
            }
            4 => {
                // 2 parents, p1 specified
                let p1 = next_id - read_next().unwrap() - 1;
                let p2 = next_id - 1;
                parents.push(vec![p1, p2]);
            }
            _ => {
                // n commits
                for _ in 0..(i - 4) {
                    let p1 = parents.len() - 1;
                    parents.push(vec![p1]);
                }
            }
        }
    }

    parents
}

fn main() {
    let parents = parse_bindag(BINDAG_MOZILLA);

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

    // Test the size, and generation speed, and ancestor calcuation speed
    // with some different segment sizes.
    for &segment_size in [4, 8, 10, 12, 14, 16, 18, 20, 22, 24, 32, 64, 128].iter() {
        let dag_dir = tempdir().unwrap();
        let mut built = false;
        bench(format!("building segment_size={}", segment_size), || {
            built = true;
            measure::Both::<measure::WallClock, String>::measure(|| {
                let mut dag = Dag::open(&dag_dir.path()).unwrap();
                let mut dag = dag.prepare_filesystem_sync().unwrap();
                let mut segment_lens = Vec::new();
                let segment_len = dag.build_flat_segments(head_id, &parents_by_id, 0).unwrap();
                segment_lens.push(segment_len);
                for level in 1..=99 {
                    // true: drop the last (potentially incomplete) segment
                    let segment_len = dag
                        .build_high_level_segments(level, segment_size, true)
                        .unwrap();
                    if segment_len == 0 {
                        break;
                    }
                    segment_lens.push(segment_len);
                }
                dag.sync().unwrap();
                let log_len = dag_dir.path().join("log").metadata().unwrap().len();
                format!("segments: {:?}  log len: {}", segment_lens, log_len)
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
                            dag.gca_one(i, j).unwrap();
                        }
                    }
                })
            },
        );
    }
}
