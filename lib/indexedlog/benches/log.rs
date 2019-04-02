// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate indexedlog;
extern crate minibench;
extern crate rand;
extern crate tempfile;

use indexedlog::log::{self, ChecksumType, IndexDef, IndexOutput, Log};
use minibench::{bench, elapsed};
use rand::{ChaChaRng, Rng};
use tempfile::tempdir;

const N: usize = 204800;

/// Generate random buffer
fn gen_buf(size: usize) -> Vec<u8> {
    let mut buf = vec![0u8; size];
    ChaChaRng::new_unseeded().fill_bytes(buf.as_mut());
    buf
}

fn main() {
    bench("log insertion", || {
        let dir = tempdir().unwrap();
        let mut log = Log::open(dir.path(), vec![]).unwrap();
        let buf = gen_buf(N * 20);
        elapsed(move || {
            for i in 0..N {
                log.append(&buf[20 * i..20 * (i + 1)]).unwrap();
            }
        })
    });

    bench("log insertion (no checksum)", || {
        let dir = tempdir().unwrap();
        let mut log = log::OpenOptions::new()
            .create(true)
            .checksum_type(ChecksumType::None)
            .open(dir.path())
            .unwrap();
        let buf = gen_buf(N * 20);
        elapsed(move || {
            for i in 0..N {
                log.append(&buf[20 * i..20 * (i + 1)]).unwrap();
            }
        })
    });

    bench("log insertion with index", || {
        let dir = tempdir().unwrap();
        let index_func = |_data: &[u8]| vec![IndexOutput::Reference(0..20)];
        let index_def = IndexDef::new("i", index_func).lag_threshold(0);
        let mut log = Log::open(dir.path(), vec![index_def]).unwrap();
        let buf = gen_buf(N * 20);
        elapsed(move || {
            for i in 0..N {
                log.append(&buf[20 * i..20 * (i + 1)]).unwrap();
            }
        })
    });

    bench("log flush", || {
        let dir = tempdir().unwrap();
        let mut log = Log::open(dir.path(), vec![]).unwrap();
        let buf = gen_buf(N * 20);
        for i in 0..N {
            log.append(&buf[20 * i..20 * (i + 1)]).unwrap();
        }
        elapsed(|| {
            log.flush().unwrap();
        })
    });

    bench("log iteration (memory)", || {
        let dir = tempdir().unwrap();
        let mut log = Log::open(dir.path(), vec![]).unwrap();
        let buf = gen_buf(N * 20);
        for i in 0..N {
            log.append(&buf[20 * i..20 * (i + 1)]).unwrap();
        }
        elapsed(move || {
            log.iter().count();
        })
    });

    bench("log iteration (disk)", || {
        let dir = tempdir().unwrap();
        let mut log = Log::open(dir.path(), vec![]).unwrap();
        let buf = gen_buf(N * 20);
        for i in 0..N {
            log.append(&buf[20 * i..20 * (i + 1)]).unwrap();
        }
        log.flush().unwrap();
        let log = Log::open(dir.path(), vec![]).unwrap();
        elapsed(move || {
            log.iter().count();
        })
    });
}
