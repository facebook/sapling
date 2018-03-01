#[macro_use]
extern crate criterion;
extern crate vlqencoding;

use criterion::Criterion;
use std::io::Cursor;
use vlqencoding::{VLQDecode, VLQEncode};

const COUNT: u64 = 16384;

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("writing via VLQEncode", |b| {
        let mut cur = Cursor::new(Vec::with_capacity(COUNT as usize * 8));
        b.iter(|| {
            cur.set_position(0);
            for i in 0..COUNT {
                cur.write_vlq(i).expect("write");
            }
        })
    });

    c.bench_function("reading via VLQDecode", |b| {
        let mut cur = Cursor::new(Vec::with_capacity(COUNT as usize * 8));
        for i in 0..COUNT {
            cur.write_vlq(i).expect("write");
        }

        b.iter(|| {
            cur.set_position(0);
            for i in 0..COUNT {
                let v: u64 = cur.read_vlq().unwrap();
                assert_eq!(v, i);
            }
        })
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
