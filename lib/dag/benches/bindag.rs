// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use vlqencoding::VLQDecode;

pub static MOZILLA: &[u8] = include_bytes!("mozilla-central.bindag");

pub fn parse_bindag(bindag: &[u8]) -> Vec<Vec<usize>> {
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
