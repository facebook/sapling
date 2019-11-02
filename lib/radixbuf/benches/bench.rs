/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use minibench::{bench, elapsed};
use rand::{ChaChaRng, RngCore};

use radixbuf::key::{FixedKey, KeyId};
use radixbuf::radix::{radix_insert, radix_lookup};

const N: usize = 204800;

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

fn main() {
    bench("index insertion", || {
        let key_buf = gen_buf(20 * N);
        elapsed(|| {
            batch_insert_radix_buf(&key_buf, N);
        })
    });

    bench("index lookup", || {
        let key_buf = gen_buf(20 * N);
        let radix_buf = batch_insert_radix_buf(&key_buf, N);
        elapsed(move || {
            for i in 0..N {
                let key_id = (i as u32 * 20).into();
                let key = FixedKey::read(&key_buf, key_id).unwrap();
                radix_lookup(&radix_buf, 0, &key, FixedKey::read, &key_buf).expect("lookup");
            }
        })
    });
}
