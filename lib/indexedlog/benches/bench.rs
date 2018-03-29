#[macro_use]
extern crate criterion;
extern crate indexedlog;
extern crate rand;
extern crate tempdir;

use criterion::Criterion;
use indexedlog::Index;
use indexedlog::base16::Base16Iter;
use rand::{ChaChaRng, Rng};
use tempdir::TempDir;

const N: usize = 20480;

/// Generate random buffer
fn gen_buf(size: usize) -> Vec<u8> {
    let mut buf = vec![0u8; size];
    ChaChaRng::new_unseeded().fill_bytes(buf.as_mut());
    buf
}

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("base16 iterating 1M bytes", |b| {
        let x = vec![4u8; 1000000];
        b.iter(|| {
            let y: u8 = Base16Iter::from_base256(&x).sum();
            assert_eq!(y, (4 * 1000000) as u8);
        })
    });

    c.bench_function("index insertion", |b| {
        let dir = TempDir::new("index").expect("TempDir::new");
        let idx = Index::open(dir.path().join("i"), 0).expect("open");
        let buf = gen_buf(N * 20);
        b.iter(move || {
            let mut idx = idx.clone().unwrap();
            for i in 0..N {
                idx.insert(&&buf[20 * i..20 * (i + 1)], i as u64)
                    .expect("insert");
            }
        });
    });

    c.bench_function("index flush", |b| {
        let dir = TempDir::new("index").expect("TempDir::new");
        let mut idx = Index::open(dir.path().join("i"), 0).expect("open");
        let buf = gen_buf(N * 20);
        for i in 0..N {
            idx.insert(&&buf[20 * i..20 * (i + 1)], i as u64)
                .expect("insert");
        }
        b.iter(|| {
            let mut idx = idx.clone().unwrap();
            idx.flush().expect("flush")
        });
    });

    c.bench_function("index lookup (memory)", |b| {
        let dir = TempDir::new("index").expect("TempDir::new");
        let mut idx = Index::open(dir.path().join("i"), 0).expect("open");
        let buf = gen_buf(N * 20);
        for i in 0..N {
            idx.insert(&&buf[20 * i..20 * (i + 1)], i as u64)
                .expect("insert");
        }
        b.iter(move || {
            let idx = idx.clone().unwrap();
            for i in 0..N {
                idx.get(&&buf[20 * i..20 * (i + 1)]).expect("lookup");
            }
        });
    });

    c.bench_function("index lookup (disk)", |b| {
        let dir = TempDir::new("index").expect("TempDir::new");
        let mut idx = Index::open(dir.path().join("i"), 0).expect("open");
        let buf = gen_buf(N * 20);
        for i in 0..N {
            idx.insert(&&buf[20 * i..20 * (i + 1)], i as u64)
                .expect("insert");
        }
        idx.flush().expect("flush");
        b.iter(move || {
            let idx = idx.clone().unwrap();
            for i in 0..N {
                idx.get(&&buf[20 * i..20 * (i + 1)]).expect("lookup");
            }
        });
    });

    c.bench_function("index clone (memory)", |b| {
        let dir = TempDir::new("index").expect("TempDir::new");
        let mut idx = Index::open(dir.path().join("i"), 0).expect("open");
        let buf = gen_buf(N * 20);
        for i in 0..N {
            idx.insert(&&buf[20 * i..20 * (i + 1)], i as u64)
                .expect("insert");
        }
        b.iter(move || {
            let mut _idx = idx.clone().unwrap();
        });
    });

    c.bench_function("index clone (disk)", |b| {
        let dir = TempDir::new("index").expect("TempDir::new");
        let mut idx = Index::open(dir.path().join("i"), 0).expect("open");
        let buf = gen_buf(N * 20);
        for i in 0..N {
            idx.insert(&&buf[20 * i..20 * (i + 1)], i as u64)
                .expect("insert");
        }
        idx.flush().expect("flush");
        b.iter(move || {
            let mut _idx = idx.clone().unwrap();
        });
    });
}

criterion_group!{
    name=benches;
    config=Criterion::default().sample_size(20);
    targets=criterion_benchmark
}
criterion_main!(benches);
