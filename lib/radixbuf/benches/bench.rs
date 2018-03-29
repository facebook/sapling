#[macro_use]
extern crate criterion;
extern crate radixbuf;
extern crate rand;

use criterion::Criterion;
use rand::{ChaChaRng, Rng};

use radixbuf::key::{FixedKey, KeyId};
use radixbuf::radix::{radix_insert, radix_lookup};

const N: usize = 20480;

/// Generate random buffer
fn gen_buf(size: usize) -> Vec<u8> {
    let mut buf = vec![0u8; size];
    ChaChaRng::new_unseeded().fill_bytes(buf.as_mut());
    buf
}

fn batch_insert_radix_buf(key_buf: &Vec<u8>, count: usize) -> Vec<u32> {
    let mut radix_buf = vec![0u32; 16];
    for i in 0..count {
        let key_id: KeyId = ((i * 20) as u32).into();
        radix_insert(&mut radix_buf, 0, key_id, FixedKey::read, key_buf).expect("insert");
    }
    radix_buf
}

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("index insertion", |b| {
        let key_buf = gen_buf(20 * N);
        b.iter(|| {
            batch_insert_radix_buf(&key_buf, N);
        })
    });

    c.bench_function("index lookup", |b| {
        let key_buf = gen_buf(20 * N);
        let radix_buf = batch_insert_radix_buf(&key_buf, N);
        b.iter(move || {
            for i in 0..N {
                let key_id = (i as u32 * 20).into();
                let key = FixedKey::read(&key_buf, key_id).unwrap();
                radix_lookup(&radix_buf, 0, &key, FixedKey::read, &key_buf).expect("lookup");
            }
        });
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
