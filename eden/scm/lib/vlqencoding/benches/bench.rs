/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io::Cursor;

use minibench::bench;
use minibench::elapsed;
use vlqencoding::VLQDecode;
use vlqencoding::VLQDecodeAt;
use vlqencoding::VLQEncode;

const COUNT: u64 = 16384;

fn main() {
    bench("writing via VLQEncode", || {
        let mut cur = Cursor::new(Vec::with_capacity(COUNT as usize * 8));
        elapsed(|| {
            cur.set_position(0);
            for i in 0..COUNT {
                cur.write_vlq(i).expect("write");
            }
        })
    });

    bench("reading via VLQDecode", || {
        let mut cur = Cursor::new(Vec::with_capacity(COUNT as usize * 8));
        for i in 0..COUNT {
            cur.write_vlq(i).expect("write");
        }

        elapsed(|| {
            cur.set_position(0);
            for i in 0..COUNT {
                let v: u64 = cur.read_vlq().unwrap();
                assert_eq!(v, i);
            }
        })
    });

    bench("reading via VLQDecodeAt", || {
        let mut cur = Vec::with_capacity(COUNT as usize * 8);
        let mut offsets = Vec::with_capacity(COUNT as usize);
        for i in 0..COUNT {
            offsets.push(cur.len());
            cur.write_vlq(i).expect("write");
        }

        elapsed(|| {
            for i in 0..COUNT {
                let offset = unsafe { *offsets.get_unchecked(i as usize) };
                let v: u64 = cur.read_vlq_at(offset).unwrap().0;
                assert_eq!(v, i);
            }
        })
    });
}
