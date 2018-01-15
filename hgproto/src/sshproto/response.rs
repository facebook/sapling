// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::fmt::Display;
use std::io::{self, Write};

use bytes::{BufMut, Bytes, BytesMut};
use futures::stream;
use futures_ext::StreamExt;

use {batch, Response, SingleResponse};
use handler::OutputStream;

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

pub fn encode(response: Response) -> OutputStream {
    let mut out = BytesMut::new();
    match response {
        Response::Batch(ref resps) => {
            let escaped_results: Vec<_> = resps
                .iter()
                .map(|resp| batch::escape(&encode_cmd(resp)))
                .collect();
            let escaped_results = Bytes::from(escaped_results.join(&b';'));
            out.reserve(10 + escaped_results.len());
            out.put_slice(format!("{}\n", escaped_results.len()).as_bytes());
            out.put(escaped_results)
        }
        Response::Single(ref resp) => encode_single(resp, &mut out),
    }
    stream::once(Ok(out.freeze())).boxify()
}

fn encode_single(response: &SingleResponse, out: &mut BytesMut) {
    let res = encode_cmd(response);
    out.reserve(10 + res.len());
    if !response.is_stream() {
        out.put_slice(format!("{}\n", res.len()).as_bytes());
    }
    out.put(res);
}

/// Encode the result of an individual command completion. This is used by both
/// single and batch responses encoding
fn encode_cmd(response: &SingleResponse) -> Bytes {
    use SingleResponse::*;

    match response {
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
