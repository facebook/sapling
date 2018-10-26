// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate indexedlog;
extern crate minibench;
extern crate rand;
extern crate tempdir;

use indexedlog::index::OpenOptions;
use minibench::{bench, elapsed};
use rand::{ChaChaRng, Rng};
use tempdir::TempDir;

const N: usize = 20480;

/// Generate random buffer
fn gen_buf(size: usize) -> Vec<u8> {
    let mut buf = vec![0u8; size];
    ChaChaRng::new_unseeded().fill_bytes(buf.as_mut());
    buf
}

/// Default open options: 4K checksum chunk
fn open_opts() -> OpenOptions {
    let mut open_opts = OpenOptions::new();
    open_opts.checksum_chunk_size(4096);
    open_opts
}

fn main() {
    bench("index insertion", || {
        let dir = TempDir::new("index").expect("TempDir::new");
        let mut idx = open_opts().open(dir.path().join("i")).expect("open");
        let buf = gen_buf(N * 20);
        elapsed(move || {
            for i in 0..N {
                idx.insert(&&buf[20 * i..20 * (i + 1)], i as u64)
                    .expect("insert");
            }
        })
    });

    bench("index flush", || {
        let dir = TempDir::new("index").expect("TempDir::new");
        let mut idx = open_opts().open(dir.path().join("i")).expect("open");
        let buf = gen_buf(N * 20);
        for i in 0..N {
            idx.insert(&&buf[20 * i..20 * (i + 1)], i as u64)
                .expect("insert");
        }
        elapsed(|| {
            idx.flush().expect("flush");
        })
    });

    bench("index lookup (memory)", || {
        let dir = TempDir::new("index").expect("TempDir::new");
        let mut idx = open_opts().open(dir.path().join("i")).expect("open");
        let buf = gen_buf(N * 20);
        for i in 0..N {
            idx.insert(&&buf[20 * i..20 * (i + 1)], i as u64)
                .expect("insert");
        }
        elapsed(move || {
            for i in 0..N {
                idx.get(&&buf[20 * i..20 * (i + 1)]).expect("lookup");
            }
        })
    });

    bench("index lookup (disk, no verify)", || {
        let dir = TempDir::new("index").expect("TempDir::new");
        let mut idx = open_opts()
            .checksum_chunk_size(0)
            .open(dir.path().join("i"))
            .expect("open");
        let buf = gen_buf(N * 20);
        for i in 0..N {
            idx.insert(&&buf[20 * i..20 * (i + 1)], i as u64)
                .expect("insert");
        }
        idx.flush().expect("flush");
        elapsed(move || {
            for i in 0..N {
                idx.get(&&buf[20 * i..20 * (i + 1)]).expect("lookup");
            }
        })
    });

    bench("index lookup (disk, verified)", || {
        let dir = TempDir::new("index").expect("TempDir::new");
        let mut idx = open_opts().open(dir.path().join("i")).expect("open");
        let buf = gen_buf(N * 20);
        for i in 0..N {
            idx.insert(&&buf[20 * i..20 * (i + 1)], i as u64)
                .expect("insert");
        }
        idx.flush().expect("flush");
        elapsed(move || {
            for i in 0..N {
                idx.get(&&buf[20 * i..20 * (i + 1)]).expect("lookup");
            }
        })
    });
}
