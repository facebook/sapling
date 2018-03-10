#[macro_use]
extern crate criterion;
extern crate indexedlog;

use criterion::Criterion;
use indexedlog::base16::Base16Iter;

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("base16 iterating 1M bytes", |b| {
        let x = vec![4u8; 1000000];
        b.iter(|| {
            let y: u8 = Base16Iter::from_base256(&x).sum();
            assert_eq!(y, (4 * 1000000) as u8);
        })
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
