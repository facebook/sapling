// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::io::{self, Write};
use std::fmt::Display;

use bytes::{BufMut, Bytes, BytesMut};

use batch;
use Response;

fn separated<I, W>(write: &mut W, iter: I, sep: &str) -> io::Result<()>
where
    I: IntoIterator,
    I::Item: Display,
    W: Write,
{
    let iter = iter.into_iter();

    let mut first = true;
    for it in iter {
        if first {
            first = false;
        } else {
            write!(write, "{}", sep)?;
        }
        write!(write, "{}", it)?;
    }
    write!(write, "\n")?;
    Ok(())
}

pub fn encode(response: &Response, out: &mut BytesMut) {
    let res = encode_cmd(response);
    out.reserve(10 + res.len());
    if !response.is_stream() {
        out.put_slice(format!("{}\n", res.len()).as_bytes());
    }
    out.put(res);
}

/// Encode the result of an individual command completion. This is used by both
/// encode and batch encoding.
pub fn encode_cmd(response: &Response) -> Bytes {
    use Response::*;

    match response {
        &Batch(ref results) => {
            let escaped_results: Vec<_> = results.iter().map(batch::escape).collect();
            Bytes::from(escaped_results.join(&b';'))
        }

        &Hello(ref map) => {
            let mut out = Vec::new();

            for (k, caps) in map.iter() {
                write!(out, "{}: {}\n", k, caps.join(" ")).expect("write to vec failed");
            }

            Bytes::from(out)
        }

        &Between(ref vecs) => {
            let mut out = Vec::new();

            for v in vecs {
                separated(&mut out, v, " ").expect("write to vec failed");
            }

            Bytes::from(out)
        }

        &Debugwireargs(ref res) => res.clone(),

        &Heads(ref set) => {
            let mut out = Vec::new();

            separated(&mut out, set, " ").expect("write to vec failed");

            Bytes::from(out)
        }

        &Known(ref knowns) => {
            let out: Vec<_> = knowns
                .iter()
                .map(|known| if *known { b'1' } else { b'0' })
                .collect();

            Bytes::from(out)
        }

        &Getbundle(ref res) => res.clone(),

        r => panic!("Response for {:?} unimplemented", r),
    }
}
