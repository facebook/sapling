/*
 * Portions Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

/* Copyright (c) 2018 Carl Lerche
 *
 * Permission is hereby granted, free of charge, to any
 * person obtaining a copy of this software and associated
 * documentation files (the "Software"), to deal in the
 * Software without restriction, including without
 * limitation the rights to use, copy, modify, merge,
 * publish, distribute, sublicense, and/or sell copies of
 * the Software, and to permit persons to whom the Software
 * is furnished to do so, subject to the following
 * conditions:
 *
 * The above copyright notice and this permission notice
 * shall be included in all copies or substantial portions
 * of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
 * ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
 * TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
 * PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
 * SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
 * CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
 * OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
 * IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
 * DEALINGS IN THE SOFTWARE.
 */
#![feature(test)]

extern crate test;

use minibytes::Bytes;
use test::Bencher;

#[bench]
fn deref_unique(b: &mut Bencher) {
    let buf = Bytes::from(vec![0; 1024]);

    b.iter(|| {
        for _ in 0..1024 {
            test::black_box(&buf[..]);
        }
    })
}

#[bench]
fn deref_shared(b: &mut Bencher) {
    let buf = Bytes::from(vec![0; 1024]);
    let _b2 = buf.clone();

    b.iter(|| {
        for _ in 0..1024 {
            test::black_box(&buf[..]);
        }
    })
}

#[bench]
fn deref_static(b: &mut Bencher) {
    let buf = Bytes::from_static(b"hello world");

    b.iter(|| {
        for _ in 0..1024 {
            test::black_box(&buf[..]);
        }
    })
}

#[bench]
fn clone_static(b: &mut Bencher) {
    let bytes =
        Bytes::from_static("hello world 1234567890 and have a good byte 0987654321".as_bytes());

    b.iter(|| {
        for _ in 0..1024 {
            test::black_box(&bytes.clone());
        }
    })
}

#[bench]
fn clone_shared(b: &mut Bencher) {
    let bytes = Bytes::from(b"hello world 1234567890 and have a good byte 0987654321".to_vec());

    b.iter(|| {
        for _ in 0..1024 {
            test::black_box(&bytes.clone());
        }
    })
}

#[bench]
fn clone_arc_vec(b: &mut Bencher) {
    use std::sync::Arc;
    let bytes = Arc::new(b"hello world 1234567890 and have a good byte 0987654321".to_vec());

    b.iter(|| {
        for _ in 0..1024 {
            test::black_box(&bytes.clone());
        }
    })
}

#[bench]
fn from_long_slice(b: &mut Bencher) {
    let data = [0u8; 128];
    b.bytes = data.len() as u64;
    b.iter(|| {
        let buf = Bytes::copy_from_slice(&data[..]);
        test::black_box(buf);
    })
}

#[bench]
fn slice_empty(b: &mut Bencher) {
    b.iter(|| {
        let b = Bytes::from(vec![17; 1024]).clone();
        for i in 0..1000 {
            test::black_box(b.slice(i % 100..i % 100));
        }
    })
}

#[bench]
fn slice_short_from_arc(b: &mut Bencher) {
    b.iter(|| {
        // `clone` is to convert to ARC
        let b = Bytes::from(vec![17; 1024]).clone();
        for i in 0..1000 {
            test::black_box(b.slice(1..2 + i % 10));
        }
    })
}
