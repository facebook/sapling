extern crate indexedlog;
extern crate minibench;
extern crate rand;
extern crate tempdir;

use indexedlog::Index;
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

fn main() {
    bench("index insertion", || {
        let dir = TempDir::new("index").expect("TempDir::new");
        let mut idx = Index::open(dir.path().join("i"), 0, true, None).expect("open");
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
        let mut idx = Index::open(dir.path().join("i"), 0, true, None).expect("open");
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
        let mut idx = Index::open(dir.path().join("i"), 0, true, None).expect("open");
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
        let mut idx = Index::open(dir.path().join("i"), 0, false, None).expect("open");
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
        let mut idx = Index::open(dir.path().join("i"), 0, true, None).expect("open");
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
